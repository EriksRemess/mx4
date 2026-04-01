use crate::Result;
use crate::device::{feature, open, req};

const ADJUSTABLE_DPI: u16 = 0x2201;
const MIN_DPI: u32 = 200;
const MAX_DPI: u32 = 8000;

pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 status dpi --json` if you want JSON".into()),
    };

    match read_status() {
        Ok(dpi) => println!("{}", format_status(dpi, json)),
        Err(_) if json => println!("null"),
        Err(_) => println!("DPI: unavailable"),
    }
    Ok(())
}

pub fn set(arg: &str) -> Result<()> {
    let (dev, idx) = open()?;
    set_value(&dev, idx, parse(arg)?)
}

pub fn parse(arg: &str) -> Result<u32> {
    let dpi: u32 = arg.trim().parse()?;

    if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
        return Err("pick a DPI value from 200 to 8000".into());
    }

    Ok(dpi)
}

pub fn read_status() -> Result<u32> {
    let (dev, idx) = open()?;
    let feature = feature(&dev, idx, ADJUSTABLE_DPI)?;
    let reply = req(&dev, idx, feature, 0x02, &[])?;
    parse_status_reply(&reply)
}

pub fn set_value(dev: &hidapi::HidDevice, idx: u8, dpi: u32) -> Result<()> {
    let feature = feature(dev, idx, ADJUSTABLE_DPI)?;
    let payload = payload(dpi);
    req(dev, idx, feature, 0x03, &payload)?;
    Ok(())
}

pub fn format_status(dpi: u32, json: bool) -> String {
    if json {
        format!(r#"{{"value":{dpi}}}"#)
    } else {
        format!("DPI: {dpi}")
    }
}

pub fn json_status() -> Result<String> {
    Ok(format_status(read_status()?, true))
}

pub fn parse_status_reply(reply: &[u8]) -> Result<u32> {
    let value = [
        0,
        *reply.get(4).ok_or("the dpi reply was too short")?,
        *reply.get(5).ok_or("the dpi reply was too short")?,
        *reply.get(6).ok_or("the dpi reply was too short")?,
    ];

    Ok(u32::from_be_bytes(value))
}

pub fn payload(dpi: u32) -> [u8; 3] {
    let bytes = dpi.to_be_bytes();
    [bytes[1], bytes[2], bytes[3]]
}

#[cfg(test)]
mod tests {
    use super::{format_status, parse, parse_status_reply, payload};

    #[test]
    fn parses_dpi_value() {
        assert_eq!(parse("2500").unwrap(), 2500);
    }

    #[test]
    fn accepts_min_dpi() {
        assert_eq!(parse("200").unwrap(), 200);
    }

    #[test]
    fn accepts_max_dpi() {
        assert_eq!(parse("8000").unwrap(), 8000);
    }

    #[test]
    fn rejects_low_dpi() {
        assert!(parse("199").is_err());
    }

    #[test]
    fn rejects_high_dpi() {
        assert!(parse("8001").is_err());
    }

    #[test]
    fn parses_dpi_status() {
        assert_eq!(
            parse_status_reply(&[0, 0, 0, 0, 0x00, 0x0A, 0x28]).unwrap(),
            2600
        );
    }

    #[test]
    fn builds_dpi_payload() {
        assert_eq!(payload(2600), [0x00, 0x0A, 0x28]);
    }

    #[test]
    fn formats_dpi_json() {
        assert_eq!(format_status(2600, true), r#"{"value":2600}"#);
    }
}
