use hidapi::{HidApi, HidDevice};

use crate::Result;

const VID: u16 = 0x046d;
const PID: u16 = 0xc548;
const PAGE: u16 = 0xff00;

pub const SHORT: u8 = 0x10;
const LONG: u8 = 0x11;
const SW_ID: u8 = 0x01;
const HIDPP_ERROR: u8 = 0x8f;
const HIDPP20_ERROR: u8 = 0xff;

pub fn open() -> Result<(HidDevice, u8)> {
    let api = HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == PAGE)
        .ok_or("couldn't find your MX receiver")?;

    Ok((
        info.open_device(&api)?,
        u8::try_from(info.interface_number())?,
    ))
}

pub fn feature(dev: &HidDevice, idx: u8, feature_id: u16) -> Result<u8> {
    let reply = req(dev, idx, 0x00, 0x00, &feature_id.to_be_bytes())?;
    let feature = *reply
        .get(4)
        .ok_or("the feature lookup reply was too short")?;

    if feature == 0 {
        return Err("that feature isn't available on this device".into());
    }

    Ok(feature)
}

pub fn req(dev: &HidDevice, idx: u8, feature: u8, function: u8, params: &[u8]) -> Result<Vec<u8>> {
    let mut drain = [0u8; 64];
    while dev.read_timeout(&mut drain, 0)? != 0 {}

    let mut pkt = [0u8; 7];
    pkt[0] = SHORT;
    pkt[1] = idx;
    pkt[2] = feature;
    pkt[3] = (function << 4) | SW_ID;
    pkt[4..4 + params.len().min(3)].copy_from_slice(&params[..params.len().min(3)]);

    write_packet(dev, &pkt, "that request didn't fully send")?;

    let mut reply = [0u8; 20];
    let mut unexpected = None;

    for _ in 0..100 {
        let len = dev.read_timeout(&mut reply, 10)?;
        if len == 0 {
            continue;
        }

        match classify_reply(&reply[..len], idx, feature, function)? {
            ReplyMatch::Matched => return Ok(reply[..len].to_vec()),
            ReplyMatch::Unexpected(message) => {
                if unexpected.is_none() {
                    unexpected = Some(message);
                }
            }
            ReplyMatch::Ignore => {}
        }
    }

    if let Some(message) = unexpected {
        return Err(message.into());
    }

    Err("the device didn't answer in time".into())
}

pub fn write_packet(dev: &HidDevice, pkt: &[u8], err: &'static str) -> Result<()> {
    if dev.write(pkt)? != pkt.len() {
        return Err(err.into());
    }

    Ok(())
}

enum ReplyMatch {
    Matched,
    Unexpected(String),
    Ignore,
}

fn classify_reply(reply: &[u8], idx: u8, feature: u8, function: u8) -> Result<ReplyMatch> {
    if reply.len() < 7 {
        return Ok(ReplyMatch::Ignore);
    }

    if reply[0] != SHORT && reply[0] != LONG {
        return Ok(ReplyMatch::Ignore);
    }

    if reply[1] != idx {
        return Ok(ReplyMatch::Ignore);
    }

    if reply[3] & 0x0f != SW_ID {
        return Ok(ReplyMatch::Ignore);
    }

    if reply[2] == HIDPP_ERROR || reply[2] == HIDPP20_ERROR {
        let code = *reply
            .get(5)
            .ok_or("the device returned a short protocol error reply")?;
        return Ok(ReplyMatch::Unexpected(format!(
            "device returned protocol error 0x{code:02x} ({})",
            hidpp_error_name(code)
        )));
    }

    if reply[2] == feature && reply[3] >> 4 == function {
        return Ok(ReplyMatch::Matched);
    }

    Ok(ReplyMatch::Unexpected(format!(
        "device returned an unexpected reply for feature 0x{feature:02x} function 0x{function:02x}: feature 0x{:02x} function 0x{:02x}",
        reply[2],
        reply[3] >> 4
    )))
}

fn hidpp_error_name(code: u8) -> &'static str {
    match code {
        0x00 => "no error",
        0x01 => "unknown or invalid sub-id",
        0x02 => "invalid arguments or address",
        0x03 => "out of range or invalid value",
        0x04 => "hardware or connection error",
        0x05 => "not allowed or too many devices",
        0x06 => "invalid feature index or already exists",
        0x07 => "invalid function id or busy",
        0x08 => "busy or unknown device",
        0x09 => "resource error",
        0x0a => "request unavailable",
        0x0b => "invalid parameter value",
        0x0c => "wrong pin code",
        _ => "unknown error",
    }
}

#[cfg(test)]
mod tests {
    use super::{ReplyMatch, classify_reply};

    #[test]
    fn matches_expected_reply() {
        let reply = [0x10, 0x07, 0x0b, 0x21, 0, 0, 0];
        assert!(matches!(
            classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap(),
            ReplyMatch::Matched
        ));
    }

    #[test]
    fn surfaces_hidpp20_error_reply() {
        let reply = [0x10, 0x07, 0xff, 0x21, 0x02, 0x09, 0];
        let ReplyMatch::Unexpected(message) = classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap()
        else {
            panic!("expected protocol error");
        };
        assert!(message.contains("protocol error 0x09"));
        assert!(message.contains("resource error"));
    }

    #[test]
    fn surfaces_unexpected_reply_shape() {
        let reply = [0x10, 0x07, 0x0c, 0x31, 0, 0, 0];
        let ReplyMatch::Unexpected(message) = classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap()
        else {
            panic!("expected unexpected reply");
        };
        assert!(message.contains("unexpected reply"));
        assert!(message.contains("feature 0x0c"));
        assert!(message.contains("function 0x03"));
    }
}
