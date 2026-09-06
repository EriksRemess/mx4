//! Battery status with support for both generations exposed by Logitech devices.

use crate::Result;
use crate::device::{feature, open, req};

const BATTERY: u16 = 0x1000;
const UNIFIED_BATTERY: u16 = 0x1004;

pub struct BatteryStatus {
    pub pct: u8,
    pub charging: bool,
}

pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 battery --json` if you want JSON".into()),
    };

    let status = read_status()?;
    println!("{}", format_status(&status, json));
    Ok(())
}

pub fn print_best_effort() {
    match read_status() {
        Ok(status) => println!("{}", format_status(&status, false)),
        Err(_) => println!("Battery: unavailable"),
    }
}

pub fn json_value(status: &BatteryStatus) -> String {
    format!(
        r#"{{"level":{},"charging":{}}}"#,
        status.pct, status.charging
    )
}

pub fn json_status() -> Result<String> {
    Ok(json_value(&read_status()?))
}

pub fn read_status() -> Result<BatteryStatus> {
    let (dev, idx) = open()?;
    // Prefer the newer unified feature, but retain the older battery feature for firmware variants
    // that do not expose it. Their status methods differ, but both put status at payload byte 2.
    let (feature, unified) = match feature(&dev, idx, UNIFIED_BATTERY) {
        Ok(feature) => (feature, true),
        Err(_) => (feature(&dev, idx, BATTERY)?, false),
    };
    let reply = req(&dev, idx, feature, if unified { 1 } else { 0 }, &[])?;
    parse_status_reply(&reply, unified)
}

fn parse_status_reply(reply: &[u8], unified: bool) -> Result<BatteryStatus> {
    let pct = *reply.get(4).ok_or("the battery reply was too short")?;
    let state = *reply.get(6).ok_or("the battery reply was too short")?;
    let charging = if unified {
        matches!(state, 1..=3)
    } else {
        matches!(state, 1..=4)
    };

    Ok(BatteryStatus { pct, charging })
}

pub fn format_status(status: &BatteryStatus, json: bool) -> String {
    if json {
        json_value(status)
    } else {
        format!(
            "Battery: {}%{}",
            status.pct,
            if status.charging { " (charging)" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{BatteryStatus, format_status, parse_status_reply};

    #[test]
    fn charging_uses_status_instead_of_reserved_byte() {
        assert!(
            parse_status_reply(&[0x11, 0, 1, 0x11, 50, 4, 1, 0], true)
                .unwrap()
                .charging
        );
        assert!(
            !parse_status_reply(&[0x11, 0, 1, 0x11, 50, 4, 0, 1], true)
                .unwrap()
                .charging
        );
        assert!(
            parse_status_reply(&[0x10, 1, 1, 1, 50, 20, 1], false)
                .unwrap()
                .charging
        );
        assert!(parse_status_reply(&[0x10, 1, 1, 1, 50, 20], false).is_err());
    }

    #[test]
    fn formats_battery_json() {
        let status = BatteryStatus {
            pct: 87,
            charging: true,
        };
        assert_eq!(
            format_status(&status, true),
            r#"{"level":87,"charging":true}"#
        );
    }
}
