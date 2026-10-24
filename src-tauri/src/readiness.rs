use crate::domain::*;

/// Once a transport has been observed, reconnects never silently replace it.
pub fn bind(
    frozen: Option<&Target>,
    avd_name: Option<&str>,
    devices: &[Device],
    expected_epoch: u64,
    epoch: u64,
) -> Result<Option<Target>> {
    if epoch != expected_epoch {
        return Err(AppError::new(
            "STALE_SESSION",
            "ADB 服务会话已经变化，请重新选择设备",
        ));
    }
    if let Some(target) = frozen {
        let device = devices
            .iter()
            .find(|device| {
                device.serial == target.serial
                    && device.transport_id == target.transport_id
                    && device.server_epoch == target.server_epoch
            })
            .ok_or_else(|| {
                AppError::new(
                    "DEVICE_UNAVAILABLE",
                    "原连接已断开，等待任务不会转移到其他连接",
                )
            })?;
        if avd_name.is_some_and(|name| device.avd_name != name)
            || !matches!(device.state.as_str(), "ready" | "booting")
        {
            return Err(AppError::new(
                "DEVICE_UNAVAILABLE",
                "原目标不可用，请重新选择设备",
            ));
        }
        return Ok(Some(target.clone()));
    }
    let name = avd_name.ok_or_else(|| AppError::new("TARGET_REQUIRED", "必须明确选择设备"))?;
    let matches: Vec<_> = devices
        .iter()
        .filter(|device| device.avd_name == name)
        .collect();
    if matches.len() > 1 {
        return Err(AppError::new(
            "AMBIGUOUS_AVD",
            "模拟器会话归属不唯一，请重新检查设备",
        ));
    }
    Ok(matches.first().map(|device| Target::from(*device)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiting_binds_once_and_rejects_disconnect_reuse_and_ambiguity() {
        let device = Device {
            serial: "emulator-5554".into(),
            transport_id: "7".into(),
            server_epoch: 1,
            avd_name: "A".into(),
            state: "booting".into(),
            ..Default::default()
        };
        assert!(bind(None, Some("A"), &[], 1, 1).unwrap().is_none());
        let target = bind(None, Some("A"), std::slice::from_ref(&device), 1, 1)
            .unwrap()
            .unwrap();
        let mut replacement = device.clone();
        replacement.transport_id = "8".into();
        assert!(bind(Some(&target), Some("A"), &[replacement.clone()], 1, 1).is_err());
        assert!(bind(Some(&target), Some("A"), &[], 1, 1).is_err());
        assert!(bind(
            Some(&target),
            Some("A"),
            std::slice::from_ref(&device),
            1,
            2
        )
        .is_err());
        assert!(bind(None, Some("A"), &[device.clone(), replacement], 1, 1).is_err());
        assert!(bind(Some(&target), Some("B"), &[device], 1, 1).is_err());
    }
}
