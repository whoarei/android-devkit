use crate::domain::*;
use sha2::Digest;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn client(settings: &Settings) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(1800))
        .redirect(reqwest::redirect::Policy::limited(4));
    if !settings.proxy.is_empty() {
        let u = reqwest::Url::parse(&settings.proxy).map_err(|_| {
            AppError::new(
                "INVALID_PROXY",
                "代理应为 http://主机:端口 或 socks5://主机:端口",
            )
        })?;
        if !u.username().is_empty() || u.password().is_some() {
            return Err(AppError::new(
                "PROXY_AUTH_UNSUPPORTED",
                "P0 请使用本地无认证代理，不会把凭据写入命令行",
            ));
        }
        b = b.proxy(
            reqwest::Proxy::all(&settings.proxy)
                .map_err(|e| AppError::new("INVALID_PROXY", e.to_string()))?,
        );
    }
    b.build()
        .map_err(|e| AppError::new("NETWORK", e.to_string()))
}
pub fn assert_writable(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let probe = path.join(format!(".devkit-probe-{}", uuid::Uuid::new_v4()));
    std::fs::write(&probe, b"probe")?;
    std::fs::remove_file(probe)?;
    Ok(())
}
pub fn free_space(path: &Path) -> u64 {
    let mut ancestor = path;
    while !ancestor.exists() {
        if let Some(p) = ancestor.parent() {
            ancestor = p;
        } else {
            return 0;
        }
    }
    fs2::available_space(ancestor).unwrap_or(0)
}
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| AppError::new("ARCHIVE_INVALID", e.to_string()))?;
    if zip.len() > 100_000 {
        return Err(AppError::new("ARCHIVE_INVALID", "归档文件数超限"));
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::new("ARCHIVE_INVALID", e.to_string()))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| AppError::new("ARCHIVE_PATH", "归档包含越界路径"))?;
        if enclosed
            .components()
            .any(|p| p.as_os_str().to_string_lossy().contains(':'))
        {
            return Err(AppError::new("ARCHIVE_PATH", "归档包含非法 Windows 路径"));
        }
        if entry.unix_mode().unwrap_or(0) & 0o170000 == 0o120000 {
            return Err(AppError::new("ARCHIVE_PATH", "不接受归档符号链接"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| AppError::new("ARCHIVE_SIZE", "归档体积溢出"))?;
        if total > 8 * 1024 * 1024 * 1024 {
            return Err(AppError::new("ARCHIVE_SIZE", "引导包解压体积超限"));
        }
        let output = dest.join(enclosed);
        if entry.is_dir() {
            std::fs::create_dir_all(output)?;
        } else {
            if let Some(p) = output.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&output)?;
            std::io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(())
}
pub async fn download(
    settings: &Settings,
    pkg: &SdkPackage,
    cache: &Path,
    progress: impl Fn(u64, u64),
) -> Result<PathBuf> {
    let url = reqwest::Url::parse(&pkg.url)
        .map_err(|_| AppError::new("UNTRUSTED_SOURCE", "下载源无效"))?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some(
                "dl.google.com"
                    | "github.com"
                    | "objects.githubusercontent.com"
                    | "release-assets.githubusercontent.com"
            )
        )
    {
        return Err(AppError::new(
            "UNTRUSTED_SOURCE",
            "下载仅允许受信任的官方发行源",
        ));
    }
    if pkg.checksum.is_empty() {
        return Err(AppError::new(
            "CHECKSUM_MISSING",
            "缺少归档校验值，拒绝安装",
        ));
    }
    std::fs::create_dir_all(cache)?;
    let file = cache.join(format!("{}.zip", digest(pkg.url.as_bytes())));
    let part = file.with_extension("part");
    let mut response = client(settings)?
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?
        .error_for_status()
        .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?;
    let size = response.content_length().unwrap_or(pkg.size);
    if size > 0 && free_space(cache) < size.saturating_mul(3) {
        return Err(AppError::new("DISK_SPACE", "缓存卷空间不足"));
    }
    let mut out = std::fs::File::create(&part)?;
    let mut bytes = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?
    {
        out.write_all(&chunk)?;
        bytes += chunk.len() as u64;
        progress(bytes, size);
    }
    out.sync_all()?;
    let mut input = std::fs::File::open(&part)?;
    let mut hash = sha2::Sha256::new();
    let mut sha1 = sha1::Sha1::new();
    let mut buf = [0; 65536];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        sha1.update(&buf[..n]);
    }
    let actual = if pkg.checksum_type == "sha256" {
        format!("{:x}", hash.finalize())
    } else if pkg.checksum_type == "sha1" {
        format!("{:x}", sha1.finalize())
    } else {
        return Err(AppError::new(
            "CHECKSUM_UNSUPPORTED",
            "不支持的归档校验算法",
        ));
    };
    if !actual.eq_ignore_ascii_case(&pkg.checksum) {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::new("CHECKSUM_FAILED", "归档校验失败，文件未安装"));
    }
    std::fs::rename(part, &file)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_zip_path_traversal() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.zip");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        z.start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"bad").unwrap();
        z.finish().unwrap();
        assert!(extract_zip(&p, &d.path().join("out")).is_err());
        assert!(!d.path().join("escape").exists());
    }
}
