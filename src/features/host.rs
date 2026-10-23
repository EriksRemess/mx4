//! Easy-Switch host selection through HID++ feature `0x1814`.

use hidapi::HidDevice;

use crate::Result;
use crate::device;

const CHANGE_HOST: u16 = 0x1814;

/// Parse a one-based Easy-Switch host number and switch to it without persisting the choice.
pub fn set(arg: &str) -> Result<()> {
    let (dev, idx) = device::open()?;
    set_value(&dev, idx, parse(arg)?)
}

/// Parse a one-based Easy-Switch host number from 1 through 3.
pub fn parse(arg: &str) -> Result<u8> {
    let host: u8 = arg.trim().parse()?;

    if !(1..=3).contains(&host) {
        return Err("pick a host from 1 to 3".into());
    }

    Ok(host)
}

/// Switch to host 1, 2, or 3 using an existing device handle and device index.
///
/// Rejects other host numbers. Sends without awaiting a reply because switching disconnects the
/// current host; success confirms the write only. Does not persist the choice.
pub fn set_value(dev: &HidDevice, idx: u8, host: u8) -> Result<()> {
    if !(1..=3).contains(&host) {
        return Err("pick a host from 1 to 3".into());
    }
    let feature = device::feature(dev, idx, CHANGE_HOST)?;
    // The CLI is one-based for humans; the HID++ feature numbers its host slots from zero.
    // Switching disconnects this host, so there is no reply to wait for.
    device::send(dev, idx, feature, 0x01, &[host - 1])
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
