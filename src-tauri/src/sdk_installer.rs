//! SDK tools receive a pinned, loopback-only repository after our downloader
//! has fetched and verified the selected archive. They cannot silently refetch
//! a different archive or fall back to Google's network repositories.
use crate::{android, domain::*, process::Spec};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const EMPTY_REPO: &str = r#"<?xml version="1.0"?><sdk:sdk-repository xmlns:sdk="http://schemas.android.com/sdk/android/repo/repository2/03"/>"#;

fn pinned_xml(data: &Path, settings: &Settings, package: &SdkPackage) -> Result<String> {
    let xml = std::fs::read_to_string(crate::sdk_source::metadata_path(
        data,
        &package.metadata_key,
    )?)?;
    if digest(xml.as_bytes()) != package.metadata_key {
        return Err(AppError::new(
            "CATALOG_CHANGED",
            "仓库元数据校验失败，请刷新目录",
        ));
    }
    let parsed = roxmltree::Document::parse(&xml)
        .map_err(|e| AppError::new("CATALOG_FORMAT", e.to_string()))?;
    let root = parsed.root_element();
    let root_range = root.range();
    let open_end = xml[root_range.start..]
        .find('>')
        .ok_or_else(|| AppError::new("CATALOG_FORMAT", "仓库根节点无效"))?
        + root_range.start
        + 1;
    let open = &xml[root_range.start..open_end];
    let prefix = root
        .lookup_prefix(root.tag_name().namespace().unwrap_or(""))
        .map(|p| format!("{p}:"))
        .unwrap_or_default();
    let shared = root
        .children()
        .filter(|n| n.is_element() && matches!(n.tag_name().name(), "license" | "channel"))
        .map(|n| &xml[n.range()])
        .collect::<String>();
    let base = reqwest::Url::parse(&package.source_url)
        .map_err(|_| AppError::new("CATALOG_FORMAT", "组件仓库地址无效"))?
        .join(".")
        .unwrap();
    for node in root.children().filter(|node| {
        node.has_tag_name("remotePackage") && node.attribute("path") == Some(&package.id)
    }) {
        let selected = &xml[node.range()];
        let document = format!(
            "{open}{shared}{selected}</{prefix}{}>",
            root.tag_name().name()
        );
        let mut candidates = android::parse_catalog(&document, base.as_str(), false)?;
        let Some(candidate) = candidates.pop() else {
            continue;
        };
        let url = if crate::sdk_source::builtin(package, settings) {
            crate::sdk_source::map_archive(settings, &candidate.url)?
        } else {
            candidate.url
        };
        if candidate.version != package.version
            || candidate.checksum != package.checksum
            || candidate.checksum_type != package.checksum_type
            || candidate.size != package.size
            || url != package.url
        {
            continue;
        }
        let archive = node
            .descendants()
            .find(|n| {
                n.has_tag_name("archive") && {
                    let host = n
                        .children()
                        .find(|c| c.has_tag_name("host-os"))
                        .and_then(|n| n.text())
                        .unwrap_or("");
                    host.is_empty() || host == "windows"
                }
            })
            .ok_or_else(|| AppError::new("ARCHIVE_STRUCTURE", "缺少 Windows 归档"))?;
        let mut edits = Vec::new();
        for other in node
            .descendants()
            .filter(|n| n.has_tag_name("archive") && *n != archive)
        {
            edits.push((other.range(), String::new()));
        }
        // We downloaded the complete archive. Never let the SDK tool select
        // an incremental patch for an already-installed revision.
        for patch in archive
            .children()
            .filter(|n| matches!(n.tag_name().name(), "patch" | "patches"))
        {
            edits.push((patch.range(), String::new()));
        }
        for url in archive.descendants().filter(|n| {
            n.has_tag_name("url")
                && n.parent()
                    .is_some_and(|parent| parent.has_tag_name("complete"))
        }) {
            edits.push((url.range(), "<url>archive.zip</url>".into()));
        }
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        let offset = node.range().start;
        let mut selected = xml[node.range()].to_owned();
        for (range, value) in edits {
            selected.replace_range(range.start - offset..range.end - offset, &value);
        }
        return Ok(format!(
            "{open}{shared}{selected}</{prefix}{}>",
            root.tag_name().name()
        ));
    }
    Err(AppError::new(
        "CATALOG_CHANGED",
        "组件归档与确认的仓库元数据不一致，请刷新后重新确认",
    ))
}
fn validate_zip(path: &Path) -> Result<()> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| AppError::new("ARCHIVE_INVALID", e.to_string()))?;
    if archive.len() > 100_000 {
        return Err(AppError::new("ARCHIVE_INVALID", "归档文件数超限"));
    }
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| AppError::new("ARCHIVE_INVALID", e.to_string()))?;
        let Some(path) = entry.enclosed_name() else {
            return Err(AppError::new("ARCHIVE_PATH", "归档包含越界路径"));
        };
        if path.components().any(|part| {
            part.as_os_str().to_string_lossy().contains(':')
                || part
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(".installer")
        }) || entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(
                "ARCHIVE_PATH",
                "归档包含不支持的路径、安装状态或符号链接",
            ));
        }
    }
    Ok(())
}
struct RepositoryFiles {
    token: String,
    xml: String,
    main: bool,
    archive: Option<PathBuf>,
}
pub struct LocalRepository {
    pub base: String,
    extra: bool,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for LocalRepository {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl LocalRepository {
    pub async fn for_package(
        data: &Path,
        settings: &Settings,
        package: &SdkPackage,
        archive: PathBuf,
    ) -> Result<Self> {
        validate_zip(&archive)?;
        let xml = pinned_xml(data, settings, package)?;
        let doc = roxmltree::Document::parse(&xml)
            .map_err(|e| AppError::new("CATALOG_FORMAT", e.to_string()))?;
        let main = matches!(
            doc.root_element().tag_name().name(),
            "repository" | "sdk-repository"
        );
        Self::start(xml, main, Some(archive)).await
    }
    pub async fn empty() -> Result<Self> {
        Self::start(EMPTY_REPO.into(), true, None).await
    }
    async fn start(xml: String, main: bool, archive: Option<PathBuf>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let token = uuid::Uuid::new_v4().to_string();
        let base = format!(
            "http://127.0.0.1:{}/{token}/",
            listener.local_addr()?.port()
        );
        let files = Arc::new(RepositoryFiles {
            token,
            xml,
            main,
            archive,
        });
        let worker = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let Ok((stream,_))=accepted else {break;};
                        if requests.len()>=8 {drop(stream);continue;}
                        let files=files.clone();
                        requests.spawn(async move {let _=tokio::time::timeout(Duration::from_secs(3600),serve(stream,files)).await;});
                    }
                    _=requests.join_next(),if !requests.is_empty()=>{}
                }
            }
        });
        Ok(Self {
            base,
            extra: !main,
            worker,
        })
    }
    pub fn configure(&self, data: &Path, settings: &Settings, spec: &mut Spec) -> Result<()> {
        let home = data
            .join("sdk-install-home")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&home)?;
        // User feeds are disabled here: only the exact pinned package is visible.
        let url = format!("{}package.xml", self.base).replace(':', "\\:");
        std::fs::write(
            home.join("repositories.cfg"),
            if self.extra {
                format!(
                    "count=1\nsrc00={url}\ndisp00=Android DevKit pinned package\nenabled00=true\n"
                )
            } else {
                "count=0\n".into()
            },
        )?;
        spec.env
            .insert("ANDROID_USER_HOME".into(), home.to_string_lossy().into());
        spec.env
            .insert("SDK_TEST_BASE_URL".into(), self.base.clone());
        if settings.sdk_channel > 0 && !spec.args.iter().any(|arg| arg.starts_with("--channel=")) {
            spec.args
                .push(format!("--channel={}", settings.sdk_channel));
        }
        Ok(())
    }
}
async fn serve(mut stream: TcpStream, files: Arc<RepositoryFiles>) -> std::io::Result<()> {
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while request.len() < 8192 {
            let read = stream.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
        }
        Ok::<_, std::io::Error>(request)
    })
    .await
    .map_err(std::io::Error::other)??;
    let line = String::from_utf8_lossy(&request);
    let mut words = line.lines().next().unwrap_or("").split_whitespace();
    let method = words.next().unwrap_or("");
    let path = words.next().unwrap_or("");
    let prefix = format!("/{}/", files.token);
    let route = path.strip_prefix(&prefix).unwrap_or("");
    let allowed = matches!(method, "GET" | "HEAD") && path.starts_with(&prefix);
    if allowed && route == "archive.zip" {
        if let Some(path) = &files.archive {
            let mut file = tokio::fs::File::open(path).await?;
            let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",file.metadata().await?.len());
            stream.write_all(header.as_bytes()).await?;
            if method != "HEAD" {
                tokio::io::copy(&mut file, &mut stream).await?;
            }
            return Ok(());
        }
    }
    let body = if allowed && route == "package.xml" {
        Some(files.xml.clone())
    } else if allowed && route.starts_with("repository2-") && route.ends_with(".xml") {
        Some(if files.main {
            files.xml.clone()
        } else {
            EMPTY_REPO.into()
        })
    } else if allowed && route.starts_with("addons_list-") && route.ends_with(".xml") {
        route.strip_prefix("addons_list-").and_then(|s|s.strip_suffix(".xml")).and_then(|s|s.parse::<u8>().ok())
                .filter(|version|*version<=20).map(|version|format!("<?xml version=\"1.0\"?><sdk:sdk-addons-list xmlns:sdk=\"http://schemas.android.com/sdk/android/addons-list/{version}\"/>"))
    } else {
        None
    };
    let status = if body.is_some() {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let body = body.unwrap_or_default();
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await?;
    if method != "HEAD" {
        stream.write_all(body.as_bytes()).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture(data: &Path) -> (Settings, SdkPackage, PathBuf) {
        let archive = data.join("fixture.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file(
            "platform-tools/source.properties",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"Pkg.Revision=1.0.0\n").unwrap();
        zip.finish().unwrap();
        use sha1::Digest;
        let checksum = format!("{:x}", sha1::Sha1::digest(std::fs::read(&archive).unwrap()));
        let xml = format!(
            r#"<sdk:sdk-repository xmlns:sdk="http://schemas.android.com/sdk/android/repo/repository2/03" xmlns:generic="http://schemas.android.com/repository/android/generic/02" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><remotePackage path="platform-tools"><type-details xsi:type="generic:genericDetailsType"/><revision><major>1</major></revision><display-name>Fixture tools</display-name><archives><archive><complete><size>{}</size><checksum type="sha1">{checksum}</checksum><url>fixture.zip</url></complete><host-os>windows</host-os></archive></archives></remotePackage></sdk:sdk-repository>"#,
            std::fs::metadata(&archive).unwrap().len()
        );
        let mut settings = Settings::default();
        settings.sdk_download.kind = "tencent".into();
        let mut package = android::parse_catalog(&xml, crate::sdk_source::TENCENT, false)
            .unwrap()
            .remove(0);
        package.source_url = crate::sdk_source::sites(&settings).unwrap()[0].url.clone();
        package.metadata_key = digest(xml.as_bytes());
        let path = crate::sdk_source::metadata_path(data, &package.metadata_key).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, xml).unwrap();
        (settings, package, archive)
    }
    #[tokio::test]
    async fn installer_uses_only_pinned_metadata_and_the_already_downloaded_archive() {
        let data = tempfile::tempdir().unwrap();
        let (settings, mut package, archive) = fixture(data.path());
        let original = std::fs::read_to_string(
            crate::sdk_source::metadata_path(data.path(), &package.metadata_key).unwrap(),
        )
        .unwrap();
        let with_patch = original.replace(
            "</complete><host-os>",
            "</complete><patches><patch><url>incremental.zip</url></patch></patches><host-os>",
        );
        package.metadata_key = digest(with_patch.as_bytes());
        std::fs::write(
            crate::sdk_source::metadata_path(data.path(), &package.metadata_key).unwrap(),
            with_patch,
        )
        .unwrap();
        let repo = LocalRepository::for_package(data.path(), &settings, &package, archive.clone())
            .await
            .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let xml = client
            .get(format!("{}repository2-4.xml", repo.base))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(xml.contains("<url>archive.zip</url>"));
        assert!(!xml.contains("patches"));
        assert!(!xml.contains("incremental.zip"));
        assert!(!xml.contains("https://"));
        let bytes = client
            .get(format!("{}archive.zip", repo.base))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), std::fs::read(&archive).unwrap());
        assert_eq!(
            client
                .get(format!("{}../fixture.zip", repo.base))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        let mut spec = Spec::new("java", vec![]);
        repo.configure(data.path(), &settings, &mut spec).unwrap();
        assert_eq!(spec.env["SDK_TEST_BASE_URL"], repo.base);
        assert_eq!(
            std::fs::read_to_string(
                Path::new(&spec.env["ANDROID_USER_HOME"]).join("repositories.cfg")
            )
            .unwrap(),
            "count=0\n"
        );
        let mut tampered = package.clone();
        tampered.checksum = "b".repeat(40);
        assert!(pinned_xml(data.path(), &settings, &tampered).is_err());
    }
    #[tokio::test]
    #[ignore = "uses the existing SDK tools to install a tiny generated image into an isolated SDK; requires ANDROID_DEVKIT_TEST_TOOLS and ANDROID_DEVKIT_TEST_JAVA"]
    async fn pinned_system_image_installs_without_remote_fetch() {
        let tools = PathBuf::from(std::env::var("ANDROID_DEVKIT_TEST_TOOLS").unwrap());
        let java = std::env::var("ANDROID_DEVKIT_TEST_JAVA").unwrap();
        let data = tempfile::tempdir().unwrap();
        let sdk = data.path().join("sdk");
        std::fs::create_dir(&sdk).unwrap();
        let archive = data.path().join("image.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file(
            "x86_64/source.properties",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"Pkg.Revision=1\nAndroidVersion.ApiLevel=35\nSystemImage.Abi=x86_64\nSystemImage.TagId=default\n").unwrap();
        zip.finish().unwrap();
        use sha1::Digest;
        let checksum = format!("{:x}", sha1::Sha1::digest(std::fs::read(&archive).unwrap()));
        let xml = format!(
            r#"<sys-img:sdk-sys-img xmlns:sys-img="http://schemas.android.com/sdk/android/repo/sys-img2/03" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><remotePackage path="system-images;android-35;default;x86_64"><type-details xsi:type="sys-img:sysImgDetailsType"><api-level>35</api-level><tag><id>default</id><display>Default</display></tag><abi>x86_64</abi></type-details><revision><major>1</major></revision><display-name>Isolated image fixture</display-name><archives><archive><complete><size>{}</size><checksum type="sha1">{checksum}</checksum><url>image.zip</url></complete></archive></archives></remotePackage></sys-img:sdk-sys-img>"#,
            std::fs::metadata(&archive).unwrap().len()
        );
        let settings = Settings {
            sdk_root: sdk.to_string_lossy().into(),
            java_path: java.clone(),
            ..Default::default()
        };
        let site = crate::repository::builtins()[1].url.clone();
        let base = reqwest::Url::parse(&site).unwrap().join(".").unwrap();
        let mut package = android::parse_catalog(&xml, base.as_str(), false)
            .unwrap()
            .remove(0);
        package.source_url = site;
        package.metadata_key = digest(xml.as_bytes());
        let metadata =
            crate::sdk_source::metadata_path(data.path(), &package.metadata_key).unwrap();
        std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
        std::fs::write(metadata, xml).unwrap();
        let repo = LocalRepository::for_package(data.path(), &settings, &package, archive)
            .await
            .unwrap();
        let mut spec = Spec::new(
            java,
            vec![
                format!("-Dcom.android.sdklib.toolsdir={}", tools.display()),
                "-classpath".into(),
                tools
                    .join("lib/sdkmanager-classpath.jar")
                    .to_string_lossy()
                    .into(),
                "com.android.sdklib.tool.sdkmanager.SdkManagerCli".into(),
                format!("--sdk_root={}", sdk.display()),
                "--install".into(),
                package.id.clone(),
            ],
        );
        repo.configure(data.path(), &settings, &mut spec).unwrap();
        spec.timeout = 60;
        crate::process::run(spec).await.unwrap().success().unwrap();
        assert!(android::installed(&settings)
            .iter()
            .any(|p| p.id == package.id && p.version == package.version));
    }
    #[tokio::test]
    #[ignore = "downloads Platform-Tools from Tencent into a fresh SDK and invokes the existing SDK tools; requires ANDROID_DEVKIT_TEST_TOOLS and ANDROID_DEVKIT_TEST_JAVA"]
    async fn tencent_archive_installs_through_pinned_sdkmanager_repository() {
        let tools = PathBuf::from(std::env::var("ANDROID_DEVKIT_TEST_TOOLS").unwrap());
        let java = std::env::var("ANDROID_DEVKIT_TEST_JAVA").unwrap();
        let data = tempfile::tempdir().unwrap();
        let sdk = data.path().join("sdk");
        std::fs::create_dir(&sdk).unwrap();
        let mut settings = Settings {
            sdk_root: sdk.to_string_lossy().into(),
            java_path: java.clone(),
            ..Default::default()
        };
        settings.sdk_download.kind = "tencent".into();
        let (packages, statuses) = crate::repository::fetch(&settings, data.path())
            .await
            .unwrap();
        assert!(
            statuses.iter().all(|site| site.error.is_empty()),
            "{statuses:?}"
        );
        let package = packages.iter().find(|p| p.id == "platform-tools").unwrap();
        assert!(package.url.starts_with(crate::sdk_source::TENCENT));
        assert_eq!(package.official_status, "ready");
        let archive =
            crate::bootstrap::download(&settings, package, &data.path().join("cache"), |_, _| {})
                .await
                .unwrap();
        let repo = LocalRepository::for_package(data.path(), &settings, package, archive)
            .await
            .unwrap();
        let mut spec = Spec::new(
            java,
            vec![
                format!("-Dcom.android.sdklib.toolsdir={}", tools.display()),
                "-classpath".into(),
                tools
                    .join("lib/sdkmanager-classpath.jar")
                    .to_string_lossy()
                    .into(),
                "com.android.sdklib.tool.sdkmanager.SdkManagerCli".into(),
                format!("--sdk_root={}", sdk.display()),
                "--install".into(),
                package.id.clone(),
            ],
        );
        repo.configure(data.path(), &settings, &mut spec).unwrap();
        spec.stdin = Some("y\n".repeat(10));
        spec.timeout = 120;
        crate::process::run(spec).await.unwrap().success().unwrap();
        let installed = android::installed(&settings);
        assert!(installed
            .iter()
            .any(|p| p.id == package.id && p.version == package.version));
    }
}
