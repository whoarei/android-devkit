use crate::domain::*;
use std::collections::BTreeSet;
pub fn at_least(actual: &str, minimum: &str) -> bool {
    fn parts(v: &str) -> [u32; 3] {
        let mut values = [0; 3];
        for (i, p) in v.split('.').take(3).enumerate() {
            values[i] = p
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
        }
        values
    }
    parts(actual) >= parts(minimum)
}
pub fn resolve(
    requested: &[String],
    local: &[SdkPackage],
    catalog: &[SdkPackage],
) -> Result<Vec<String>> {
    fn visit(
        id: &str,
        local: &[SdkPackage],
        catalog: &[SdkPackage],
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        ordered: &mut Vec<String>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.into()) || visiting.len() > 100 {
            return Err(AppError::new(
                "DEPENDENCY_CYCLE",
                "组件依赖循环或超出安全上限",
            ));
        }
        if let Some(package) = catalog
            .iter()
            .find(|p| p.id == id)
            .or_else(|| local.iter().find(|p| p.id == id))
        {
            for dep in &package.dependencies {
                let minimum = package
                    .dependency_min_versions
                    .get(dep)
                    .map(String::as_str)
                    .unwrap_or("0");
                if !local
                    .iter()
                    .any(|p| p.id == *dep && at_least(&p.version, minimum))
                {
                    if let Some(remote) = catalog.iter().find(|p| p.id == *dep) {
                        if !at_least(&remote.version, minimum) {
                            return Err(AppError::new(
                                "DEPENDENCY_VERSION",
                                format!("{dep} 需要至少 {minimum}，当前稳定目录未提供满足版本"),
                            ));
                        }
                    } else {
                        return Err(AppError::new(
                            "DEPENDENCY_UNAVAILABLE",
                            format!("缺少 {dep}（至少 {minimum}）的可安装元数据"),
                        ));
                    }
                    visit(dep, local, catalog, visiting, done, ordered)?;
                }
            }
        } else {
            return Err(AppError::new("PACKAGE_UNKNOWN", format!("未找到组件 {id}")));
        }
        visiting.remove(id);
        done.insert(id.into());
        ordered.push(id.into());
        Ok(())
    }
    let mut ordered = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut done = BTreeSet::new();
    for id in requested {
        visit(id, local, catalog, &mut visiting, &mut done, &mut ordered)?;
    }
    Ok(ordered)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrades_known_old_dependency_before_image() {
        let old = SdkPackage {
            id: "emulator".into(),
            version: "36.1.6".into(),
            installed: true,
            ..Default::default()
        };
        let new = SdkPackage {
            id: "emulator".into(),
            version: "37.2.12".into(),
            ..Default::default()
        };
        let image = SdkPackage {
            id: "system-images;android-37;google_apis;x86_64".into(),
            dependencies: vec!["emulator".into()],
            dependency_min_versions: std::collections::BTreeMap::from([(
                "emulator".into(),
                "37.0.0".into(),
            )]),
            ..Default::default()
        };
        assert_eq!(
            resolve(
                std::slice::from_ref(&image.id),
                &[old],
                &[new, image.clone()]
            )
            .unwrap(),
            vec!["emulator", &image.id]
        );
        assert!(at_least("36.10.1", "36.9.0"));
        assert!(!at_least("36.1.6", "37"));
    }
}
