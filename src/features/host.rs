use crate::Result;
use crate::device::{feature, open, req};

const CHANGE_HOST: u16 = 0x1814;

pub fn set(arg: &str) -> Result<()> {
    let (dev, idx) = open()?;
    set_value(&dev, idx, parse(arg)?)
}

pub fn parse(arg: &str) -> Result<u8> {
    let host: u8 = arg.trim().parse()?;

    if !(1..=3).contains(&host) {
        return Err("pick a host from 1 to 3".into());
    }

    Ok(host)
}

pub fn set_value(dev: &hidapi::HidDevice, idx: u8, host: u8) -> Result<()> {
    let feature = feature(dev, idx, CHANGE_HOST)?;
    req(dev, idx, feature, 0x01, &[host - 1])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_host_number() {
        assert_eq!(parse("2").unwrap(), 2);
    }

    #[test]
    fn rejects_invalid_host_number() {
        assert!(parse("4").is_err());
    }
}
