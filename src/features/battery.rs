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

    match read_status() {
        Ok(status) => println!("{}", format_status(&status, json)),
        Err(_) if json => println!("null"),
        Err(_) => println!("Battery: unavailable"),
    }
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
    let (feature, unified) = match feature(&dev, idx, UNIFIED_BATTERY) {
        Ok(feature) => (feature, true),
        Err(_) => (feature(&dev, idx, BATTERY)?, false),
    };
    let reply = req(&dev, idx, feature, if unified { 1 } else { 0 }, &[])?;
    let pct = *reply.get(4).ok_or("the battery reply was too short")?;
    let charging = if unified {
        matches!(reply.get(7).copied(), Some(1..=3))
    } else {
        matches!(reply.get(6).copied(), Some(1..=4))
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
    use super::{BatteryStatus, format_status};

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
