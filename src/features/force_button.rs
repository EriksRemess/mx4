use crate::Result;
use crate::device::{feature, open, req};

const FORCE_SENSING_BUTTON: u16 = 0x19c0;

pub struct ForceButtonInfo {
    pub changeable: bool,
    pub min_value: u16,
    pub max_value: u16,
}

pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 status force-button --json` if you want JSON".into()),
    };

    match read_status() {
        Ok((value, info)) => println!("{}", format_status(value, &info, json)),
        Err(_) if json => println!("null"),
        Err(_) => println!("Force Button: unavailable"),
    }
    Ok(())
}

pub fn set(arg: &str) -> Result<()> {
    let (dev, idx) = open()?;
    let feature = feature(&dev, idx, FORCE_SENSING_BUTTON)?;
    let info = read_info(&dev, idx, feature)?;
    let value = parse(arg, info.min_value, info.max_value)?;

    if !info.changeable {
        return Err("that force button isn't changeable".into());
    }

    req(
        &dev,
        idx,
        feature,
        0x03,
        &[0, (value >> 8) as u8, value as u8],
    )?;
    Ok(())
}

fn parse(arg: &str, min_value: u16, max_value: u16) -> Result<u16> {
    let value: u16 = arg.trim().parse()?;

    if !(min_value..=max_value).contains(&value) {
        return Err(format!(
            "pick a force-button value from {} to {}",
            min_value, max_value
        )
        .into());
    }

    Ok(value)
}

fn read_info(dev: &hidapi::HidDevice, idx: u8, feature_idx: u8) -> Result<ForceButtonInfo> {
    let reply = req(dev, idx, feature_idx, 0x01, &[0])?;
    parse_info_reply(&reply)
}

fn read_current(dev: &hidapi::HidDevice, idx: u8, feature_idx: u8) -> Result<u16> {
    let reply = req(dev, idx, feature_idx, 0x02, &[0])?;
    let bytes = reply
        .get(4..6)
        .ok_or("the force-button current reply was too short")?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

pub fn read_status() -> Result<(u16, ForceButtonInfo)> {
    let (dev, idx) = open()?;
    let feature_idx = feature(&dev, idx, FORCE_SENSING_BUTTON)?;
    let info = read_info(&dev, idx, feature_idx)?;
    let current = read_current(&dev, idx, feature_idx)?;
    Ok((current, info))
}

fn parse_info_reply(reply: &[u8]) -> Result<ForceButtonInfo> {
    let data = reply
        .get(4..12)
        .ok_or("the force-button reply was too short")?;
    let changeable = u16::from_be_bytes([data[0], data[1]]) & 0x01 != 0;
    let max_value = u16::from_be_bytes([data[4], data[5]]);
    let min_value = u16::from_be_bytes([data[6], data[7]]);

    if min_value > max_value {
        return Err("the force-button reply was invalid".into());
    }

    Ok(ForceButtonInfo {
        changeable,
        min_value,
        max_value,
    })
}

pub fn format_status(current: u16, info: &ForceButtonInfo, json: bool) -> String {
    if json {
        format!(
            r#"{{"value":{},"changeable":{},"min":{},"max":{}}}"#,
            current, info.changeable, info.min_value, info.max_value
        )
    } else {
        format!(
            "Force Button: {}{} (range {}..={})",
            current,
            if info.changeable { "" } else { " (fixed)" },
            info.min_value,
            info.max_value
        )
    }
}

pub fn json_status() -> Result<String> {
    let (current, info) = read_status()?;
    Ok(format_status(current, &info, true))
}

#[cfg(test)]
mod tests {
    use super::{format_status, parse, parse_info_reply};

    #[test]
    fn parses_force_button_info() {
        let info = parse_info_reply(&[0, 0, 0, 0, 0x00, 0x01, 0x12, 0x34, 0x27, 0x10, 0x00, 0xc8])
            .unwrap();
        assert!(info.changeable);
        assert_eq!(info.min_value, 200);
        assert_eq!(info.max_value, 10000);
    }

    #[test]
    fn parses_force_button_value() {
        assert_eq!(parse("6309", 200, 10000).unwrap(), 6309);
    }

    #[test]
    fn rejects_force_button_value_out_of_range() {
        assert!(parse("199", 200, 10000).is_err());
    }

    #[test]
    fn formats_force_button_status() {
        let info = parse_info_reply(&[0, 0, 0, 0, 0x00, 0x01, 0x12, 0x34, 0x27, 0x10, 0x00, 0xc8])
            .unwrap();
        assert_eq!(
            format_status(4310, &info, false),
            "Force Button: 4310 (range 200..=10000)"
        );
    }

    #[test]
    fn formats_force_button_json() {
        let info = parse_info_reply(&[0, 0, 0, 0, 0x00, 0x01, 0x12, 0x34, 0x27, 0x10, 0x00, 0xc8])
            .unwrap();
        assert_eq!(
            format_status(4310, &info, true),
            r#"{"value":4310,"changeable":true,"min":200,"max":10000}"#
        );
    }
}
