//! Shared HID++ transport and device discovery.
//!
//! Bolt uses seven-byte short reports addressed through the receiver. A directly connected
//! Bluetooth MX Master 4 uses twenty-byte long reports and device index zero. Higher-level features
//! use the same request API; this module handles those transport differences.

use hidapi::{BusType, DeviceInfo, HidApi, HidDevice};
use std::collections::HashSet;

use crate::Result;

const VID: u16 = 0x046d;
const BOLT_RECEIVER_PID: u16 = 0xc548;
const MX_MASTER_4_BT_PID: u16 = 0xb042;
const BOLT_HIDPP_PAGE: u16 = 0xff00;
const BT_HIDPP_PAGE: u16 = 0xff43;
const BT_DIRECT_INDEX: u8 = 0x00;

pub const SHORT: u8 = 0x10;
const LONG: u8 = 0x11;
// Older mx4 daemons use ID 1 and do not take our transport lock. Keep their replies separate
// while serializing current clients, which all use ID 2, through transport_lock.
const SW_ID: u8 = 0x02;
const HIDPP_ERROR: u8 = 0x8f;
const HIDPP20_ERROR: u8 = 0xff;

pub fn open() -> Result<(HidDevice, u8)> {
    let api = HidApi::new()?;
    if let Some(info) = api.device_list().find(|d| is_bluetooth_hidpp(d)) {
        return Ok((info.open_device(&api)?, BT_DIRECT_INDEX));
    }

    let mut seen = HashSet::new();
    let mut last_error = None;
    for info in api.device_list().filter(|d| is_bolt_hidpp(d)) {
        // hidapi may enumerate multiple usages for the same HID node.
        if !seen.insert(info.path()) {
            continue;
        }
        let result: Result<(HidDevice, u8)> = (|| {
            let dev = info.open_device(&api)?;
            let idx = find_bolt_mouse(|idx, feature, function, params| {
                req(&dev, idx, feature, function, params)
            })?;
            Ok((dev, idx))
        })();
        match result {
            Ok(device) => return Ok(device),
            Err(err) if err.is::<std::io::Error>() => return Err(err),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        "couldn't find an MX Master 4 HID++ device over Bluetooth or a Logi Bolt receiver".into()
    }))
}

pub fn open_bolt_receiver() -> Result<Option<HidDevice>> {
    let api = HidApi::new()?;
    // `open()` deliberately prefers a directly connected Bluetooth mouse. Firmware status also
    // needs the receiver itself, so discover it independently instead of reusing that preference.
    let Some(info) = api.device_list().find(|d| is_bolt_hidpp(d)) else {
        return Ok(None);
    };

    Ok(Some(info.open_device(&api)?))
}

pub fn feature(dev: &HidDevice, idx: u8, feature_id: u16) -> Result<u8> {
    // HID++ 2.0 feature IDs are stable, but the one-byte runtime index is assigned by each device.
    // The root feature at index zero resolves that mapping.
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
    let _lock = crate::transport_lock::acquire()?;
    // Remove delayed notifications or replies from an earlier request before matching a new one.
    let mut drain = [0u8; 64];
    while dev.read_timeout(&mut drain, 0)? != 0 {}

    send_unlocked(dev, idx, feature, function, params)?;

    let mut reply = [0u8; 20];
    let mut unexpected = None;

    // Notifications share the HID endpoint with replies. Ignore unrelated traffic while retaining
    // the first well-formed but unexpected reply for a useful timeout error.
    for _ in 0..100 {
        let len = dev.read_timeout(&mut reply, 10)?;
        if len == 0 {
            continue;
        }

        match classify_reply(&reply[..len], idx, feature, function)? {
            ReplyMatch::Matched => return Ok(reply[..len].to_vec()),
            ReplyMatch::Rejected(message) => return Err(message.into()),
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

    Err(RequestTimeout.into())
}

/// Send a short feature request without waiting for a reply, for operations that disconnect.
pub fn send(dev: &HidDevice, idx: u8, feature: u8, function: u8, params: &[u8]) -> Result<()> {
    let _lock = crate::transport_lock::acquire()?;
    send_unlocked(dev, idx, feature, function, params)
}

fn send_unlocked(dev: &HidDevice, idx: u8, feature: u8, function: u8, params: &[u8]) -> Result<()> {
    let pkt = request_packet(idx, feature, function, params)?;
    write_packet(dev, &pkt, "that request didn't fully send")
}

fn request_packet(idx: u8, feature: u8, function: u8, params: &[u8]) -> Result<[u8; 7]> {
    if function > 0x0f || params.len() > 3 {
        return Err("invalid short HID++ request".into());
    }
    let mut pkt = [SHORT, idx, feature, (function << 4) | SW_ID, 0, 0, 0];
    pkt[4..4 + params.len()].copy_from_slice(params);
    Ok(pkt)
}

pub fn write_packet(dev: &HidDevice, pkt: &[u8], err: &'static str) -> Result<()> {
    let pkt = wire_packet(report_id(dev), pkt);

    if dev.write(&pkt)? != pkt.len() {
        return Err(err.into());
    }

    Ok(())
}

fn is_bluetooth_hidpp(info: &DeviceInfo) -> bool {
    is_bluetooth_mx4(info) && info.usage_page() == BT_HIDPP_PAGE
}

fn is_bluetooth_mx4(info: &DeviceInfo) -> bool {
    info.vendor_id() == VID
        && info.product_id() == MX_MASTER_4_BT_PID
        && matches!(info.bus_type(), BusType::Bluetooth)
}

fn is_bolt_hidpp(info: &DeviceInfo) -> bool {
    info.vendor_id() == VID
        && info.product_id() == BOLT_RECEIVER_PID
        && info.usage_page() == BOLT_HIDPP_PAGE
}

fn find_bolt_mouse(mut request: impl FnMut(u8, u8, u8, &[u8]) -> Result<Vec<u8>>) -> Result<u8> {
    // Receiver slots are independent of USB interfaces. DEVICE_FW_VERSION's model ID table
    // identifies the hardware even when its current transport is Bolt rather than Bluetooth.
    let mut first_error = None;
    for idx in 1..=7 {
        let identified = (|| -> Result<bool> {
            let lookup = request(idx, 0, 0, &[0, 3])?;
            let feature = *lookup.get(4).ok_or("short feature lookup reply")?;
            if feature == 0 {
                return Ok(false);
            }
            is_mx4_model(&request(idx, feature, 0, &[])?)
        })();
        match identified {
            Ok(true) => return Ok(idx),
            Ok(false) => {}
            Err(err) if is_absent_slot(err.as_ref()) => {}
            // Local filesystem/lock failures and HID I/O failures cannot be fixed by trying another slot.
            Err(err) if err.is::<std::io::Error>() || err.is::<hidapi::HidError>() => {
                return Err(err);
            }
            Err(err) => {
                first_error.get_or_insert(err);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| {
        "couldn't find a connected MX Master 4 in the Bolt receiver's device slots".into()
    }))
}

fn is_absent_slot(err: &(dyn std::error::Error + 'static)) -> bool {
    err.is::<RequestTimeout>()
        || err.downcast_ref::<ProtocolError>().is_some_and(|err| {
            err.report == HIDPP_ERROR && err.code == 0x09 // Bolt's unknown-device reply.
        })
}

fn is_mx4_model(reply: &[u8]) -> Result<bool> {
    let transports = *reply.get(10).ok_or("short device information reply")?;
    let models = reply.get(11..17).ok_or("short device model ID table")?;
    let mut ids = models.as_chunks::<2>().0.iter();
    for flag in [0x01, 0x02, 0x04, 0x08] {
        if transports & flag != 0 {
            let id = ids.next().ok_or("invalid device model ID table")?;
            if flag <= 0x02 && u16::from_be_bytes(*id) == MX_MASTER_4_BT_PID {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn report_id(dev: &HidDevice) -> u8 {
    match dev.get_device_info() {
        Ok(info) if is_bluetooth_mx4(&info) => LONG,
        _ => SHORT,
    }
}

fn wire_packet(report_id: u8, pkt: &[u8]) -> Vec<u8> {
    if report_id != LONG {
        return pkt.to_vec();
    }

    // Bluetooth accepts only the long report shape even when a feature needs three parameters or
    // fewer. Preserve the common header and zero-fill the unused payload.
    let mut long = vec![0u8; 20];
    let len = pkt.len().min(long.len());
    long[..len].copy_from_slice(&pkt[..len]);
    long[0] = LONG;
    long
}

#[derive(Debug)]
struct ProtocolError {
    report: u8,
    code: u8,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "device returned protocol error 0x{:02x} ({})",
            self.code,
            hidpp_error_name(self.report, self.code)
        )
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug)]
struct RequestTimeout;

impl std::fmt::Display for RequestTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the device didn't answer in time")
    }
}

impl std::error::Error for RequestTimeout {}

enum ReplyMatch {
    Matched,
    Rejected(ProtocolError),
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

    if reply[2] == HIDPP_ERROR || reply[2] == HIDPP20_ERROR {
        // Error replies move the original feature and function/software ID into bytes three and
        // four. Match both fields before attributing the error to this request.
        if reply[3] != feature || reply[4] != ((function << 4) | SW_ID) {
            return Ok(ReplyMatch::Ignore);
        }

        let code = *reply
            .get(5)
            .ok_or("the device returned a short protocol error reply")?;
        return Ok(ReplyMatch::Rejected(ProtocolError {
            report: reply[2],
            code,
        }));
    }

    // The software ID distinguishes our synchronous response from traffic generated by another
    // HID++ client using the same endpoint.
    if reply[3] & 0x0f != SW_ID {
        return Ok(ReplyMatch::Ignore);
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

fn hidpp_error_name(error_type: u8, code: u8) -> &'static str {
    if error_type == HIDPP20_ERROR {
        match code {
            0x00 => "no error",
            0x01 => "unknown error",
            0x02 => "invalid arguments",
            0x03 => "out of range",
            0x04 => "hardware error",
            0x05 => "not allowed",
            0x06 => "invalid feature index",
            0x07 => "invalid function id",
            0x08 => "busy",
            0x09 => "unsupported",
            _ => "unknown error",
        }
    } else {
        match code {
            0x00 => "no error",
            0x01 => "unknown error",
            0x02 => "invalid sub-id",
            0x03 => "invalid address",
            0x04 => "invalid value",
            0x05 => "connection failed",
            0x06 => "too many devices",
            0x07 => "already exists",
            0x08 => "busy",
            0x09 => "unknown device",
            0x0a => "resource error",
            0x0b => "request unavailable",
            0x0c => "wrong pin code",
            _ => "unknown error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LONG, ReplyMatch, SHORT, classify_reply, wire_packet};

    fn model_reply(pid: u16) -> Vec<u8> {
        let mut reply = vec![0; 20];
        reply[0] = LONG;
        reply[10] = 0x06; // Bluetooth LE and receiver transport IDs.
        reply[11..13].copy_from_slice(&pid.to_be_bytes());
        reply
    }

    #[test]
    fn discovers_mouse_in_pairing_slot_instead_of_usb_interface() {
        for mouse_slot in [1, 3, 7] {
            let found = super::find_bolt_mouse(|idx, feature, function, params| {
                assert_eq!(function, 0);
                if idx == 2 {
                    return Err("slot is offline".into());
                }
                if feature == 0 {
                    assert_eq!(params, &[0, 3]);
                    Ok(vec![SHORT, idx, 0, 1, 5, 0, 0])
                } else {
                    assert_eq!(feature, 5);
                    assert!(params.is_empty());
                    Ok(model_reply(if idx == mouse_slot { 0xb042 } else { 0xb034 }))
                }
            })
            .unwrap();
            assert_eq!(found, mouse_slot);
        }
    }

    #[test]
    fn rejects_receivers_without_an_identifiable_mx4() {
        assert!(super::find_bolt_mouse(|_, _, _, _| Ok(vec![0; 7])).is_err());
        assert!(super::find_bolt_mouse(|_, _, _, _| Err("offline".into())).is_err());
        assert!(super::is_mx4_model(&[0; 7]).is_err());
        let mut reply = model_reply(0xb042);
        reply[10] = 0x04; // Matching bytes in a WPID are not a matching Bluetooth model.
        assert!(!super::is_mx4_model(&reply).unwrap());
        reply[10] = 0x07;
        reply[11..15].copy_from_slice(&[0, 0, 0xb0, 0x42]);
        assert!(super::is_mx4_model(&reply).unwrap());
    }

    #[test]
    fn discovery_propagates_lock_errors_immediately() {
        let mut calls = 0;
        let err = super::find_bolt_mouse(|_, _, _, _| {
            calls += 1;
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cannot open transport.lock",
            )
            .into())
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(err.to_string().contains("transport.lock"));
    }

    #[test]
    fn discovery_keeps_real_error_after_skipping_absent_slots() {
        let err = super::find_bolt_mouse(|idx, _, _, _| match idx {
            1 => Err(super::RequestTimeout.into()),
            2 => Err("malformed device model ID table".into()),
            _ => Err(super::ProtocolError {
                report: super::HIDPP_ERROR,
                code: 0x09,
            }
            .into()),
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "malformed device model ID table");
        assert!(!super::is_absent_slot(&super::ProtocolError {
            report: super::HIDPP20_ERROR,
            code: 0x09
        }));
    }

    #[test]
    fn builds_host_switch_for_both_transports() {
        let packet = super::request_packet(3, 5, 1, &[1]).unwrap();
        assert_eq!(packet, [SHORT, 3, 5, 0x12, 1, 0, 0]);
        assert_eq!(
            &wire_packet(LONG, &packet)[..7],
            &[LONG, 3, 5, 0x12, 1, 0, 0]
        );
        assert!(super::request_packet(3, 5, 16, &[]).is_err());
        assert!(super::request_packet(3, 5, 1, &[0; 4]).is_err());
    }

    #[test]
    fn matches_expected_reply() {
        let reply = [0x10, 0x07, 0x0b, 0x22, 0, 0, 0];
        assert!(matches!(
            classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap(),
            ReplyMatch::Matched
        ));
    }

    #[test]
    fn ignores_legacy_client_lookup_and_error_replies() {
        // Captured concurrent DPI/haptic lookups share feature/function zero. The old daemon's
        // reply must never supply the new client's runtime feature index, even if it arrives first.
        for reply in [
            [LONG, 2, 0, 0x01, 0x0b, 0, 0],
            [SHORT, 2, 0xff, 0, 0x01, 2, 0],
        ] {
            assert!(matches!(
                classify_reply(&reply, 2, 0, 0).unwrap(),
                ReplyMatch::Ignore
            ));
        }
        let reply = [LONG, 2, 0, 0x02, 0x14, 0, 2];
        assert!(matches!(
            classify_reply(&reply, 2, 0, 0).unwrap(),
            ReplyMatch::Matched
        ));
    }

    #[test]
    fn surfaces_hidpp20_error_reply() {
        let reply = [0x10, 0x07, 0xff, 0x0b, 0x22, 0x09, 0];
        let ReplyMatch::Rejected(message) = classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap()
        else {
            panic!("expected protocol error");
        };
        assert!(message.to_string().contains("protocol error 0x09"));
        assert!(message.to_string().contains("unsupported"));
    }

    #[test]
    fn ignores_error_reply_for_another_request() {
        let reply = [0x10, 0x07, 0xff, 0x0c, 0x32, 0x09, 0];
        assert!(matches!(
            classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap(),
            ReplyMatch::Ignore
        ));
    }

    #[test]
    fn surfaces_unexpected_reply_shape() {
        let reply = [0x10, 0x07, 0x0c, 0x32, 0, 0, 0];
        let ReplyMatch::Unexpected(message) = classify_reply(&reply, 0x07, 0x0b, 0x02).unwrap()
        else {
            panic!("expected unexpected reply");
        };
        assert!(message.contains("unexpected reply"));
        assert!(message.contains("feature 0x0c"));
        assert!(message.contains("function 0x03"));
    }

    #[test]
    fn keeps_short_packets_for_short_report_devices() {
        let pkt = [SHORT, 0, 1, 2, 3, 4, 5];
        assert_eq!(wire_packet(SHORT, &pkt), pkt);
    }

    #[test]
    fn expands_short_packets_for_long_report_devices() {
        let pkt = [SHORT, 0, 1, 2, 3, 4, 5];
        let wire = wire_packet(LONG, &pkt);
        assert_eq!(wire.len(), 20);
        assert_eq!(wire[0], LONG);
        assert_eq!(&wire[1..7], &pkt[1..7]);
        assert!(wire[7..].iter().all(|b| *b == 0));
    }
}
