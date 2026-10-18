use crate::{app::App, domain::*};
use sha2::Digest;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

impl App {
    pub fn prepare_apk(&self, path: &Path) -> Result<ApkInfo> {
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| !s.eq_ignore_ascii_case("apk"))
            .unwrap_or(true)
        {
            return Err(AppError::new(
                "APK_ONLY",
                "首版仅支持完整单 APK，不支持 AAB、APKS 或 XAPK",
            ));
        }
        let meta = path.metadata()?;
        if !meta.is_file() || meta.len() > 1024 * 1024 * 1024 {
            return Err(AppError::new("APK_SIZE", "APK 应为不超过 1 GiB 的普通文件"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.data.join("apks");
        std::fs::create_dir_all(&dir)?;
        let staged = dir.join(format!("{id}.apk"));
        let mut input = std::fs::File::open(path)?;
        let mut out = std::fs::File::create(&staged)?;
        let mut hash = sha2::Sha256::new();
        let mut buf = [0; 65536];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
        }
        out.sync_all()?;
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&staged)?)
            .map_err(|_| AppError::new("INVALID_APK", "文件不是有效 APK / ZIP"))?;
        if archive.len() > 100_000 {
            return Err(AppError::new("INVALID_APK", "APK 文件条目超限"));
        }
        let mut abis = Vec::new();
        for i in 0..archive.len() {
            let entry = archive
                .by_index(i)
                .map_err(|_| AppError::new("INVALID_APK", "无法读取 APK"))?;
            if let Some(name) = entry
                .name()
                .strip_prefix("lib/")
                .and_then(|n| n.split_once('/'))
                .map(|(abi, _)| abi.to_string())
            {
                if !abis.contains(&name) {
                    abis.push(name);
                }
            }
        }
        let manifest = archive
            .by_name("AndroidManifest.xml")
            .map_err(|_| AppError::new("INVALID_APK", "缺少 AndroidManifest.xml"))?;
        if manifest.size() > MAX_MANIFEST_BYTES as u64 {
            return Err(AppError::new("INVALID_APK", "Manifest 体积异常"));
        }
        let mut bytes = Vec::new();
        manifest
            .take((MAX_MANIFEST_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(AppError::new("MANIFEST_LIMIT", "Manifest 体积超限"));
        }
        let metadata = parse_manifest(&bytes);
        let mut apk = ApkInfo {
            id,
            path: staged.to_string_lossy().into(),
            file_name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            size: meta.len(),
            sha256: format!("{:x}", hash.finalize()),
            abis,
            ..Default::default()
        };
        match metadata {
            Ok(attrs) => {
                apk.package = attrs.get("manifest:package").cloned().unwrap_or_default();
                apk.version = attrs
                    .get("manifest:versionName")
                    .cloned()
                    .unwrap_or_else(|| {
                        attrs
                            .get("manifest:versionCode")
                            .cloned()
                            .unwrap_or_default()
                    });
                apk.min_sdk = attrs
                    .get("uses-sdk:minSdkVersion")
                    .cloned()
                    .unwrap_or_default();
                if attrs.contains_key("manifest:split") {
                    return Err(AppError::new("SPLIT_APK", "这是分包 APK，请选择完整单 APK"));
                }
            }
            Err(e) => {
                apk.warning = format!(
                    "Manifest 部分字段无法读取：{}；安装结果由设备判断",
                    e.message
                )
            }
        }
        std::fs::write(
            dir.join(format!("{}.json", apk.id)),
            serde_json::to_vec(&apk)?,
        )?;
        self.apks
            .lock()
            .unwrap()
            .insert(apk.id.clone(), apk.clone());
        Ok(apk)
    }
}
fn u16_at(b: &[u8], i: usize) -> Result<usize> {
    b.get(i..i + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)
        .ok_or_else(|| AppError::new("MANIFEST_FORMAT", "二进制 XML 越界"))
}
fn u32_at(b: &[u8], i: usize) -> Result<usize> {
    b.get(i..i + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        .ok_or_else(|| AppError::new("MANIFEST_FORMAT", "二进制 XML 越界"))
}
fn len8(b: &[u8], i: &mut usize) -> Result<usize> {
    let a = *b
        .get(*i)
        .ok_or_else(|| AppError::new("MANIFEST_FORMAT", "字符串越界"))? as usize;
    *i += 1;
    if a & 0x80 != 0 {
        let c = *b
            .get(*i)
            .ok_or_else(|| AppError::new("MANIFEST_FORMAT", "字符串越界"))?
            as usize;
        *i += 1;
        Ok(((a & 0x7f) << 8) | c)
    } else {
        Ok(a)
    }
}
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;
struct DecodeBudget(usize);
impl DecodeBudget {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.0 = self
            .0
            .checked_sub(bytes)
            .ok_or_else(|| AppError::new("MANIFEST_LIMIT", "Manifest 解码内容超限"))?;
        Ok(())
    }
}
fn string_pool(chunk: &[u8], header: usize, budget: &mut DecodeBudget) -> Result<Vec<Arc<str>>> {
    if header < 28 {
        return Err(AppError::new("MANIFEST_FORMAT", "无效字符串池头"));
    }
    let count = u32_at(chunk, 8)?;
    let styles = u32_at(chunk, 12)?;
    let flags = u32_at(chunk, 16)?;
    let start = u32_at(chunk, 20)?;
    let style_start = u32_at(chunk, 24)?;
    let end = if style_start == 0 {
        chunk.len()
    } else {
        style_start
    };
    if count > 100_000
        || styles > 100_000
        || header + (count + styles) * 4 > start
        || start > end
        || end > chunk.len()
    {
        return Err(AppError::new("MANIFEST_FORMAT", "字符串池表越界或超限"));
    }
    let data = &chunk[start..end];
    let mut interned: HashMap<usize, Arc<str>> = HashMap::new();
    let mut strings = Vec::with_capacity(count);
    for n in 0..count {
        let offset = u32_at(chunk, header + n * 4)?;
        if let Some(text) = interned.get(&offset) {
            strings.push(text.clone());
            continue;
        }
        let mut p = offset;
        let text: Arc<str> = if flags & 0x100 != 0 {
            let _chars = len8(data, &mut p)?;
            let size = len8(data, &mut p)?;
            let raw = data
                .get(p..p + size)
                .ok_or_else(|| AppError::new("MANIFEST_FORMAT", "字符串越界"))?;
            if data.get(p + size) != Some(&0) {
                return Err(AppError::new("MANIFEST_FORMAT", "字符串缺少终止符"));
            }
            match std::str::from_utf8(raw) {
                Ok(text) => {
                    budget.charge(text.len())?;
                    Arc::from(text)
                }
                Err(_) => {
                    budget.charge(size * 3)?;
                    Arc::from(String::from_utf8_lossy(raw).as_ref())
                }
            }
        } else {
            let mut length = u16_at(data, p)?;
            p += 2;
            if length & 0x8000 != 0 {
                length = ((length & 0x7fff) << 16) | u16_at(data, p)?;
                p += 2;
            }
            let raw = data
                .get(p..p + length * 2)
                .ok_or_else(|| AppError::new("MANIFEST_FORMAT", "字符串越界"))?;
            if u16_at(data, p + length * 2)? != 0 {
                return Err(AppError::new("MANIFEST_FORMAT", "字符串缺少终止符"));
            }
            // Reserve the worst case UTF-8 size before allocating. Decode from
            // the input directly, without a second Vec<u16> copy.
            budget.charge(length * 3)?;
            let text: String = char::decode_utf16(
                raw.chunks_exact(2)
                    .map(|s| u16::from_le_bytes([s[0], s[1]])),
            )
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
            Arc::from(text)
        };
        interned.insert(offset, text.clone());
        strings.push(text);
    }
    Ok(strings)
}
pub fn parse_manifest(b: &[u8]) -> Result<std::collections::BTreeMap<String, String>> {
    if b.len() > MAX_MANIFEST_BYTES {
        return Err(AppError::new("MANIFEST_LIMIT", "Manifest 体积超限"));
    }
    if u16_at(b, 0)? != 3 || u16_at(b, 2)? < 8 {
        return Err(AppError::new("MANIFEST_FORMAT", "不是 Android 二进制 XML"));
    }
    let mut offset = u16_at(b, 2)?;
    let mut strings: Option<Vec<Arc<str>>> = None;
    let mut budget = DecodeBudget(MAX_DECODED_BYTES);
    let mut attrs = std::collections::BTreeMap::new();
    while offset + 8 <= b.len() {
        let kind = u16_at(b, offset)?;
        let header = u16_at(b, offset + 2)?;
        let size = u32_at(b, offset + 4)?;
        if size < header
            || header < 8
            || offset.checked_add(size).filter(|v| *v <= b.len()).is_none()
        {
            return Err(AppError::new("MANIFEST_FORMAT", "无效 XML 块大小"));
        }
        let chunk = &b[offset..offset + size];
        if kind == 1 {
            if strings.is_some() {
                return Err(AppError::new("MANIFEST_FORMAT", "重复字符串池"));
            }
            strings = Some(string_pool(chunk, header, &mut budget)?);
        }
        if kind == 0x102 {
            let ext = header;
            let pool = strings.as_deref().unwrap_or_default();
            let get = |index: usize| pool.get(index).map(|s| &**s).unwrap_or("");
            let name = get(u32_at(chunk, ext + 4)?);
            let start = u16_at(chunk, ext + 8)?;
            let step = u16_at(chunk, ext + 10)?;
            let count = u16_at(chunk, ext + 12)?;
            if step < 20 || count > 4096 || ext + start + count * step > chunk.len() {
                return Err(AppError::new("MANIFEST_FORMAT", "无效属性表"));
            }
            for n in 0..count {
                let p = ext + start + n * step;
                let key = get(u32_at(chunk, p + 4)?);
                let raw = u32_at(chunk, p + 8)?;
                let number;
                let value = if raw < pool.len() {
                    get(raw)
                } else {
                    let data = u32_at(chunk, p + 16)?;
                    match chunk.get(p + 15) {
                        Some(3) => get(data),
                        Some(0x10 | 0x11) => {
                            number = data.to_string();
                            &number
                        }
                        _ => "",
                    }
                };
                budget.charge(name.len() + 1 + key.len() + value.len())?;
                attrs.insert(format!("{name}:{key}"), value.to_owned());
            }
        }
        offset += size;
    }
    Ok(attrs)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn put16(b: &mut [u8], at: usize, value: usize) {
        b[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }
    fn put32(b: &mut [u8], at: usize, value: usize) {
        b[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    fn pool(offsets: &[usize], data: &[u8]) -> Vec<u8> {
        let start = 28 + offsets.len() * 4;
        let mut b = vec![0; start + data.len()];
        let size = b.len();
        put16(&mut b, 0, 1);
        put16(&mut b, 2, 28);
        put32(&mut b, 4, size);
        put32(&mut b, 8, offsets.len());
        put32(&mut b, 16, 0x100);
        put32(&mut b, 20, start);
        for (i, offset) in offsets.iter().enumerate() {
            put32(&mut b, 28 + i * 4, *offset);
        }
        b[start..].copy_from_slice(data);
        b
    }
    fn manifest_with_attributes(value: &str, count: usize) -> Vec<u8> {
        let mut data = vec![];
        let mut offsets = vec![];
        for value in ["manifest", "package", value] {
            offsets.push(data.len());
            let n = value.len();
            for _ in 0..2 {
                if n > 127 {
                    data.extend([(0x80 | (n >> 8)) as u8, n as u8]);
                } else {
                    data.push(n as u8);
                }
            }
            data.extend(value.bytes());
            data.push(0);
        }
        let p = pool(&offsets, &data);
        let mut element = vec![0; 36 + count * 20];
        let size = element.len();
        put16(&mut element, 0, 0x102);
        put16(&mut element, 2, 16);
        put32(&mut element, 4, size);
        put32(&mut element, 20, 0);
        put16(&mut element, 24, 20);
        put16(&mut element, 26, 20);
        put16(&mut element, 28, count);
        for n in 0..count {
            let a = 36 + n * 20;
            put32(&mut element, a, u32::MAX as usize);
            put32(&mut element, a + 4, 1);
            put32(&mut element, a + 8, 2);
            put16(&mut element, a + 12, 8);
            element[a + 15] = 3;
            put32(&mut element, a + 16, 2);
        }
        let mut xml = vec![0; 8];
        put16(&mut xml, 0, 3);
        put16(&mut xml, 2, 8);
        put32(&mut xml, 4, 8 + p.len() + element.len());
        xml.extend(p);
        xml.extend(element);
        xml
    }
    #[test]
    fn aliased_strings_share_one_decoding_allocation() {
        let mut data = vec![0xa0, 0, 0xa0, 0];
        data.extend(std::iter::repeat_n(b'x', 8192));
        data.push(0);
        let chunk = pool(&vec![0; 4096], &data);
        let mut budget = DecodeBudget(MAX_DECODED_BYTES);
        let strings = string_pool(&chunk, 28, &mut budget).unwrap();
        assert_eq!(budget.0, MAX_DECODED_BYTES - 8192);
        assert!(Arc::ptr_eq(&strings[0], &strings[4095]));
        assert_eq!(strings[4095].len(), 8192);
    }
    #[test]
    fn attribute_copy_amplification_is_rejected_by_total_budget() {
        let bytes = manifest_with_attributes(&"x".repeat(8192), 4096);
        assert!(bytes.len() < 100_000);
        assert_eq!(parse_manifest(&bytes).unwrap_err().code, "MANIFEST_LIMIT");
    }
    #[test]
    fn bounded_manifest_still_extracts_application_package() {
        let bytes = manifest_with_attributes("dev.example", 1);
        assert_eq!(
            parse_manifest(&bytes).unwrap()["manifest:package"],
            "dev.example"
        );
    }
    #[test]
    fn lossy_decode_and_string_boundaries_are_checked_before_allocation() {
        let mut budget = DecodeBudget(8);
        let bad_utf8 = pool(&[0], &[3, 3, 0xff, 0xff, 0xff, 0]);
        assert_eq!(
            string_pool(&bad_utf8, 28, &mut budget).unwrap_err().code,
            "MANIFEST_LIMIT"
        );
        let truncated = pool(&[0], &[3, 3, b'a']);
        assert_eq!(
            string_pool(&truncated, 28, &mut DecodeBudget(1024))
                .unwrap_err()
                .code,
            "MANIFEST_FORMAT"
        );
    }
    #[test]
    fn malformed_binary_xml_is_bounded() {
        for b in [
            vec![],
            vec![3, 0, 8, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0, 0, 0, 0],
            vec![3, 0, 255, 255],
        ] {
            let _ = parse_manifest(&b);
        }
    }
}
