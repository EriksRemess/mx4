use crate::Result;
use crate::device::{feature, open, req};

const SMART_SHIFT_ENHANCED: u16 = 0x2111;
const HIRES_WHEEL: u16 = 0x2121;
const THUMB_WHEEL: u16 = 0x2150;

const WHEEL_DIVERT_MASK: u8 = 0x01;
const WHEEL_RESOLUTION_MASK: u8 = 0x02;
const WHEEL_INVERT_MASK: u8 = 0x04;
const THUMB_MODE_MASK: [u8; 2] = [0x01, 0x00];
const THUMB_INVERT_MASK: [u8; 2] = [0x00, 0x01];
const WHEEL_RATCHET_FREE: u8 = 0x01;
const WHEEL_RATCHET_RATCHET: u8 = 0x02;

pub struct SmartShiftState {
    pub ratchet: u8,
    pub ratchet_speed: u8,
    pub force: u8,
}

pub struct HiresWheelStatus {
    pub invert: bool,
    pub resolution: bool,
    pub divert: bool,
}

pub struct ThumbWheelStatus {
    pub invert: bool,
    pub divert: bool,
}

pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 status wheel --json` if you want JSON".into()),
    };

    match read_status() {
        Ok((smart_shift, hires)) => {
            println!("{}", format_status(&smart_shift, &hires, json));
        }
        Err(_) if json => println!("null"),
        Err(_) => println!("Wheel: unavailable"),
    }
    Ok(())
}

pub fn thumb_status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 status thumb-wheel --json` if you want JSON".into()),
    };

    match read_thumb_status() {
        Ok(status) => println!("{}", format_thumb_status(&status, json)),
        Err(_) if json => println!("null"),
        Err(_) => println!("Thumb Wheel: unavailable"),
    }
    Ok(())
}

pub fn set(setting: &str, value: &str) -> Result<()> {
    let (dev, idx) = open()?;

    match setting {
        "ratchet" => {
            let mut state = read_smart_shift_state(&dev, idx)?;
            state.ratchet = parse_ratchet(value)?;
            write_smart_shift_state(&dev, idx, &state)
        }
        "ratchet-speed" | "smart-shift" => {
            let mut state = read_smart_shift_state(&dev, idx)?;
            state.ratchet_speed = parse_ratchet_speed(value)?;
            write_smart_shift_state(&dev, idx, &state)
        }
        "force" => {
            let mut state = read_smart_shift_state(&dev, idx)?;
            state.force = parse_force(value)?;
            write_smart_shift_state(&dev, idx, &state)
        }
        "invert" => set_hires_flag(&dev, idx, WHEEL_INVERT_MASK, parse_toggle(value)?),
        "resolution" => set_hires_flag(&dev, idx, WHEEL_RESOLUTION_MASK, parse_toggle(value)?),
        "divert" => set_hires_flag(&dev, idx, WHEEL_DIVERT_MASK, parse_toggle(value)?),
        _ => Err(
            "try `mx4 set wheel ratchet free`, `mx4 set wheel ratchet-speed 10`, `mx4 set wheel force 75`, `mx4 set wheel invert on`, `mx4 set wheel resolution on`, or `mx4 set wheel divert on`"
                .into(),
        ),
    }
}

pub fn set_thumb(setting: &str, value: &str) -> Result<()> {
    let (dev, idx) = open()?;

    match setting {
        "invert" => set_thumb_flag(&dev, idx, THUMB_INVERT_MASK, parse_toggle(value)?),
        "divert" => set_thumb_flag(&dev, idx, THUMB_MODE_MASK, parse_toggle(value)?),
        _ => Err("try `mx4 set thumb-wheel invert on` or `mx4 set thumb-wheel divert on`".into()),
    }
}

fn parse_ratchet(arg: &str) -> Result<u8> {
    match arg.trim() {
        "free" => Ok(WHEEL_RATCHET_FREE),
        "ratchet" => Ok(WHEEL_RATCHET_RATCHET),
        _ => Err("pick `free` or `ratchet`".into()),
    }
}

fn parse_ratchet_speed(arg: &str) -> Result<u8> {
    let value: u8 = arg.trim().parse()?;

    if value > 50 {
        return Err("pick a ratchet-speed value from 0 to 50".into());
    }

    Ok(value)
}

fn parse_force(arg: &str) -> Result<u8> {
    let value: u8 = arg.trim().parse()?;

    if !(1..=100).contains(&value) {
        return Err("pick a wheel force value from 1 to 100".into());
    }

    Ok(value)
}

fn parse_toggle(arg: &str) -> Result<bool> {
    match arg.trim() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("pick `on` or `off`".into()),
    }
}

fn read_smart_shift_state(dev: &hidapi::HidDevice, idx: u8) -> Result<SmartShiftState> {
    let feature_idx = feature(dev, idx, SMART_SHIFT_ENHANCED)?;
    let reply = req(dev, idx, feature_idx, 0x01, &[])?;
    parse_smart_shift_reply(&reply)
}

fn read_hires_status(dev: &hidapi::HidDevice, idx: u8) -> Result<HiresWheelStatus> {
    let feature_idx = feature(dev, idx, HIRES_WHEEL)?;
    let reply = req(dev, idx, feature_idx, 0x01, &[])?;
    parse_hires_reply(&reply)
}

pub fn read_thumb_status() -> Result<ThumbWheelStatus> {
    let (dev, idx) = open()?;
    let feature_idx = feature(&dev, idx, THUMB_WHEEL)?;
    let reply = req(&dev, idx, feature_idx, 0x01, &[])?;
    parse_thumb_reply(&reply)
}

pub fn read_status() -> Result<(SmartShiftState, HiresWheelStatus)> {
    let (dev, idx) = open()?;
    let smart_shift = read_smart_shift_state(&dev, idx)?;
    let hires = read_hires_status(&dev, idx)?;
    Ok((smart_shift, hires))
}

fn write_smart_shift_state(
    dev: &hidapi::HidDevice,
    idx: u8,
    state: &SmartShiftState,
) -> Result<()> {
    let feature_idx = feature(dev, idx, SMART_SHIFT_ENHANCED)?;
    req(
        dev,
        idx,
        feature_idx,
        0x02,
        &[state.ratchet, state.ratchet_speed, state.force],
    )?;
    Ok(())
}

fn parse_smart_shift_reply(reply: &[u8]) -> Result<SmartShiftState> {
    Ok(SmartShiftState {
        ratchet: *reply.get(4).ok_or("the wheel reply was too short")?,
        ratchet_speed: *reply.get(5).ok_or("the wheel reply was too short")?,
        force: *reply.get(6).ok_or("the wheel reply was too short")?,
    })
}

fn parse_hires_reply(reply: &[u8]) -> Result<HiresWheelStatus> {
    let flags = *reply.get(4).ok_or("the wheel reply was too short")?;
    Ok(HiresWheelStatus {
        invert: flags & WHEEL_INVERT_MASK != 0,
        resolution: flags & WHEEL_RESOLUTION_MASK != 0,
        divert: flags & WHEEL_DIVERT_MASK != 0,
    })
}

fn parse_thumb_reply(reply: &[u8]) -> Result<ThumbWheelStatus> {
    let bytes = reply
        .get(4..6)
        .ok_or("the thumb-wheel reply was too short")?;
    let flags = [bytes[0], bytes[1]];
    Ok(ThumbWheelStatus {
        invert: flags[1] & THUMB_INVERT_MASK[1] != 0,
        divert: flags[0] & THUMB_MODE_MASK[0] != 0,
    })
}

fn set_hires_flag(dev: &hidapi::HidDevice, idx: u8, mask: u8, enabled: bool) -> Result<()> {
    let feature_idx = feature(dev, idx, HIRES_WHEEL)?;
    let mut reply = req(dev, idx, feature_idx, 0x01, &[])?;
    let flags = reply.get_mut(4).ok_or("the wheel reply was too short")?;
    *flags = apply_mask_u8(*flags, mask, enabled);
    req(dev, idx, feature_idx, 0x02, &[*flags])?;
    Ok(())
}

fn set_thumb_flag(dev: &hidapi::HidDevice, idx: u8, mask: [u8; 2], enabled: bool) -> Result<()> {
    let feature_idx = feature(dev, idx, THUMB_WHEEL)?;
    let reply = req(dev, idx, feature_idx, 0x01, &[])?;
    let bytes = reply
        .get(4..6)
        .ok_or("the thumb-wheel reply was too short")?;
    let payload = apply_mask_bytes([bytes[0], bytes[1]], mask, enabled);
    req(dev, idx, feature_idx, 0x02, &payload)?;
    Ok(())
}

fn apply_mask_u8(value: u8, mask: u8, enabled: bool) -> u8 {
    if enabled { value | mask } else { value & !mask }
}

fn apply_mask_bytes(value: [u8; 2], mask: [u8; 2], enabled: bool) -> [u8; 2] {
    if enabled {
        [value[0] | mask[0], value[1] | mask[1]]
    } else {
        [value[0] & !mask[0], value[1] & !mask[1]]
    }
}

pub fn format_status(
    smart_shift: &SmartShiftState,
    hires: &HiresWheelStatus,
    json: bool,
) -> String {
    if json {
        format!(
            r#"{{"ratchet":"{}","ratchet_speed":{},"force":{},"invert":{},"resolution":{},"divert":{}}}"#,
            format_ratchet(smart_shift.ratchet),
            smart_shift.ratchet_speed,
            smart_shift.force,
            hires.invert,
            hires.resolution,
            hires.divert,
        )
    } else {
        format!(
            "Wheel: ratchet={}, ratchet-speed={}, force={}, invert={}, resolution={}, divert={}",
            format_ratchet(smart_shift.ratchet),
            smart_shift.ratchet_speed,
            smart_shift.force,
            format_toggle(hires.invert),
            format_toggle(hires.resolution),
            format_toggle(hires.divert),
        )
    }
}

pub fn format_thumb_status(status: &ThumbWheelStatus, json: bool) -> String {
    if json {
        format!(
            r#"{{"invert":{},"divert":{}}}"#,
            status.invert, status.divert
        )
    } else {
        format!(
            "Thumb Wheel: invert={}, divert={}",
            format_toggle(status.invert),
            format_toggle(status.divert),
        )
    }
}

pub fn json_status() -> Result<String> {
    let (smart_shift, hires) = read_status()?;
    Ok(format_status(&smart_shift, &hires, true))
}

pub fn json_thumb_status() -> Result<String> {
    Ok(format_thumb_status(&read_thumb_status()?, true))
}

fn format_ratchet(value: u8) -> &'static str {
    match value {
        WHEEL_RATCHET_FREE => "free",
        WHEEL_RATCHET_RATCHET => "ratchet",
        _ => "unknown",
    }
}

fn format_toggle(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::{
        SmartShiftState, apply_mask_bytes, apply_mask_u8, format_status, format_thumb_status,
        parse_force, parse_hires_reply, parse_ratchet, parse_ratchet_speed,
        parse_smart_shift_reply, parse_thumb_reply, parse_toggle,
    };

    #[test]
    fn parses_ratchet_modes() {
        assert_eq!(parse_ratchet("free").unwrap(), 1);
        assert_eq!(parse_ratchet("ratchet").unwrap(), 2);
    }

    #[test]
    fn parses_ratchet_speed_value() {
        assert_eq!(parse_ratchet_speed("10").unwrap(), 10);
    }

    #[test]
    fn rejects_ratchet_speed_value_out_of_range() {
        assert!(parse_ratchet_speed("51").is_err());
    }

    #[test]
    fn parses_force_value() {
        assert_eq!(parse_force("75").unwrap(), 75);
    }

    #[test]
    fn rejects_force_value_out_of_range() {
        assert!(parse_force("101").is_err());
    }

    #[test]
    fn parses_toggle_values() {
        assert!(parse_toggle("on").unwrap());
        assert!(!parse_toggle("off").unwrap());
    }

    #[test]
    fn parses_smart_shift_reply() {
        let state = parse_smart_shift_reply(&[0, 0, 0, 0, 1, 10, 75]).unwrap();
        assert_eq!(
            (state.ratchet, state.ratchet_speed, state.force),
            (1, 10, 75)
        );
    }

    #[test]
    fn parses_hires_reply() {
        let status = parse_hires_reply(&[0, 0, 0, 0, 0x07]).unwrap();
        assert!(status.invert);
        assert!(status.resolution);
        assert!(status.divert);
    }

    #[test]
    fn parses_thumb_reply() {
        let status = parse_thumb_reply(&[0, 0, 0, 0, 0x01, 0x01]).unwrap();
        assert!(status.invert);
        assert!(status.divert);
    }

    #[test]
    fn applies_single_byte_mask() {
        assert_eq!(apply_mask_u8(0x00, 0x04, true), 0x04);
        assert_eq!(apply_mask_u8(0x07, 0x04, false), 0x03);
    }

    #[test]
    fn applies_multi_byte_mask() {
        assert_eq!(apply_mask_bytes([0, 0], [0, 1], true), [0, 1]);
        assert_eq!(apply_mask_bytes([1, 1], [1, 0], false), [0, 1]);
    }

    #[test]
    fn smart_shift_state_shape_is_stable() {
        let state = SmartShiftState {
            ratchet: 1,
            ratchet_speed: 10,
            force: 75,
        };
        assert_eq!(
            (state.ratchet, state.ratchet_speed, state.force),
            (1, 10, 75)
        );
    }

    #[test]
    fn formats_wheel_status() {
        let smart_shift = SmartShiftState {
            ratchet: 1,
            ratchet_speed: 10,
            force: 75,
        };
        let hires = parse_hires_reply(&[0, 0, 0, 0, 0x04]).unwrap();
        assert_eq!(
            format_status(&smart_shift, &hires, false),
            "Wheel: ratchet=free, ratchet-speed=10, force=75, invert=on, resolution=off, divert=off"
        );
    }

    #[test]
    fn formats_thumb_status() {
        let thumb = parse_thumb_reply(&[0, 0, 0, 0, 0x00, 0x01]).unwrap();
        assert_eq!(
            format_thumb_status(&thumb, false),
            "Thumb Wheel: invert=on, divert=off"
        );
    }

    #[test]
    fn formats_wheel_json() {
        let smart_shift = SmartShiftState {
            ratchet: 2,
            ratchet_speed: 10,
            force: 75,
        };
        let hires = parse_hires_reply(&[0, 0, 0, 0, 0x03]).unwrap();
        assert_eq!(
            format_status(&smart_shift, &hires, true),
            r#"{"ratchet":"ratchet","ratchet_speed":10,"force":75,"invert":false,"resolution":true,"divert":true}"#
        );
    }

    #[test]
    fn formats_thumb_json() {
        let thumb = parse_thumb_reply(&[0, 0, 0, 0, 0x01, 0x00]).unwrap();
        assert_eq!(
            format_thumb_status(&thumb, true),
            r#"{"invert":false,"divert":true}"#
        );
    }
}
