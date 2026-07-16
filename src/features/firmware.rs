//! Firmware information for both halves of an MX Master 4 setup.
//!
//! The mouse exposes a discoverable HID++ 2.0 feature, regardless of whether it is connected over
//! Bluetooth or Bolt. The Bolt receiver is a separate HID++ 1.0 device with an indexed firmware
//! table, so it must be opened and queried independently.

use hidapi::HidDevice;

use crate::Result;
use crate::device::{SHORT, feature, open, open_bolt_receiver, req, write_packet};

const DEVICE_FW_VERSION: u16 = 0x0003;
const RECEIVER_INDEX: u8 = 0xff;
const GET_LONG_REGISTER: u8 = 0x83;
const RECEIVER_FW_INFORMATION: u8 = 0xf4;
const HIDPP_ERROR: u8 = 0x8f;

/// The role of one firmware entity reported by the mouse or receiver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareKind {
    Firmware,
    Bootloader,
    Hardware,
    Other,
}

impl FirmwareKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Firmware => "firmware",
            Self::Bootloader => "bootloader",
            Self::Hardware => "hardware",
            Self::Other => "other",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Firmware => "Firmware",
            Self::Bootloader => "Bootloader",
            Self::Hardware => "Hardware",
            Self::Other => "Other",
        }
    }
}

/// One independently versioned firmware, bootloader, or hardware entity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirmwareEntity {
    pub kind: FirmwareKind,
    pub name: Option<String>,
    pub version: String,
}

/// Best-effort firmware results for the mouse and an optional Bolt receiver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirmwareStatus {
    pub mouse: Option<Vec<FirmwareEntity>>,
    pub receiver: Option<Vec<FirmwareEntity>>,
}

pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 firmware --json` if you want JSON".into()),
    };
    let status = read_status()?;
    println!("{}", format_status(&status, json));
    Ok(())
}

pub fn read_status() -> Result<FirmwareStatus> {
    // A Bluetooth mouse can be available without a receiver, and a receiver can remain plugged in
    // while its mouse is switched away. Preserve whichever half answered instead of failing both.
    let mouse = read_mouse().ok();
    let receiver = read_receiver().ok().flatten();

    if mouse.is_none() && receiver.is_none() {
        return Err("couldn't read firmware information from the mouse or Bolt receiver".into());
    }

    Ok(FirmwareStatus { mouse, receiver })
}

pub fn read_mouse() -> Result<Vec<FirmwareEntity>> {
    let (dev, idx) = open()?;
    // HID++ 2.0 feature IDs are resolved through the root feature before their runtime index can be
    // used. Function 0 returns the entity count; function 1 returns one entity by index.
    let feature = feature(&dev, idx, DEVICE_FW_VERSION)?;
    let info = req(&dev, idx, feature, 0x00, &[])?;
    let count = *info
        .get(4)
        .ok_or("the device firmware information reply was too short")?;
    let mut entities = Vec::with_capacity(usize::from(count));

    for entity_idx in 0..count {
        let reply = req(&dev, idx, feature, 0x01, &[entity_idx])?;
        entities.push(parse_mouse_entity(&reply)?);
    }

    Ok(entities)
}

pub fn read_receiver() -> Result<Option<Vec<FirmwareEntity>>> {
    let Some(dev) = open_bolt_receiver()? else {
        return Ok(None);
    };
    let mut entities = Vec::new();

    // Bolt's 0xf4 register has no entity-count field. Current C548 receivers expose the first three
    // slots; entity types not useful as standalone versions (for example the radio stack) are
    // intentionally ignored by the parser.
    for entity_idx in 0..3 {
        let reply = receiver_entity(&dev, entity_idx)?;
        if let Some(entity) = parse_receiver_entity(&reply)? {
            entities.push(entity);
        }
    }

    Ok(Some(entities))
}

pub fn format_status(status: &FirmwareStatus, json: bool) -> String {
    if json {
        return format!(
            r#"{{"mouse":{},"receiver":{}}}"#,
            json_entities(status.mouse.as_deref()),
            json_entities(status.receiver.as_deref())
        );
    }

    let mut lines = Vec::new();
    text_entities(&mut lines, "Mouse", status.mouse.as_deref());
    text_entities(&mut lines, "Bolt receiver", status.receiver.as_deref());
    lines.join("\n")
}

fn receiver_entity(dev: &HidDevice, entity_idx: u8) -> Result<Vec<u8>> {
    let mut drain = [0u8; 64];
    while dev.read_timeout(&mut drain, 0)? != 0 {}

    // HID++ 1.0 long-register read:
    // report ID, receiver index, GET_LONG_REGISTER, register, entity index, padding.
    let packet = [
        SHORT,
        RECEIVER_INDEX,
        GET_LONG_REGISTER,
        RECEIVER_FW_INFORMATION,
        entity_idx,
        0,
        0,
    ];
    write_packet(
        dev,
        &packet,
        "the receiver firmware request didn't fully send",
    )?;

    let mut reply = [0u8; 20];
    for _ in 0..100 {
        let len = dev.read_timeout(&mut reply, 10)?;
        if len < 7 || reply[1] != RECEIVER_INDEX {
            continue;
        }
        if reply[2] == HIDPP_ERROR
            && reply[3] == GET_LONG_REGISTER
            && reply[4] == RECEIVER_FW_INFORMATION
        {
            return Err(
                format!("the Bolt receiver returned HID++ error 0x{:02x}", reply[5]).into(),
            );
        }
        if reply[2] == GET_LONG_REGISTER && reply[3] == RECEIVER_FW_INFORMATION {
            return Ok(reply[..len].to_vec());
        }
    }

    Err("the Bolt receiver didn't answer the firmware request in time".into())
}

fn parse_mouse_entity(reply: &[u8]) -> Result<FirmwareEntity> {
    // HID++ 2.0 response payload starts at byte 4. The low nibble is the entity kind; firmware
    // entities then contain a three-byte name, major/minor bytes, and a big-endian build number.
    let kind_id = *reply
        .get(4)
        .ok_or("the mouse firmware entity reply was too short")?
        & 0x0f;
    let kind = firmware_kind(kind_id);

    if kind == FirmwareKind::Hardware {
        let version = reply
            .get(5)
            .ok_or("the mouse hardware revision reply was too short")?
            .to_string();
        return Ok(FirmwareEntity {
            kind,
            name: None,
            version,
        });
    }

    if reply.len() < 12 {
        return Err("the mouse firmware entity reply was too short".into());
    }
    let name = ascii_name(&reply[5..8]);
    let build = u16::from_be_bytes([reply[10], reply[11]]);

    Ok(FirmwareEntity {
        kind,
        name,
        version: format_version(reply[8], reply[9], build),
    })
}

fn parse_receiver_entity(reply: &[u8]) -> Result<Option<FirmwareEntity>> {
    if reply.len() < 9 {
        return Err("the Bolt receiver firmware entity reply was too short".into());
    }
    // Receiver replies use the payload type to select the conventional Logitech prefix. Version
    // components follow immediately and use the same hexadecimal representation as mouse entities.
    let (kind, name) = match reply[4] {
        0 => (FirmwareKind::Firmware, "MPR"),
        1 => (FirmwareKind::Bootloader, "BOT"),
        _ => return Ok(None),
    };
    let build = u16::from_be_bytes([reply[7], reply[8]]);

    Ok(Some(FirmwareEntity {
        kind,
        name: Some(name.to_string()),
        version: format_version(reply[5], reply[6], build),
    }))
}

fn firmware_kind(value: u8) -> FirmwareKind {
    match value {
        0 => FirmwareKind::Firmware,
        1 => FirmwareKind::Bootloader,
        2 => FirmwareKind::Hardware,
        _ => FirmwareKind::Other,
    }
}

fn format_version(major: u8, minor: u8, build: u16) -> String {
    // Logitech displays each component as hexadecimal and omits the build suffix when it is zero.
    let version = format!("{major:02X}.{minor:02X}");
    if build == 0 {
        version
    } else {
        format!("{version}.B{build:04X}")
    }
}

fn ascii_name(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    if bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn text_entities(lines: &mut Vec<String>, heading: &str, entities: Option<&[FirmwareEntity]>) {
    lines.push(format!("{heading}:"));
    match entities {
        Some(entities) if !entities.is_empty() => {
            for entity in entities {
                let name = entity
                    .name
                    .as_deref()
                    .map(|name| format!(" {name}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "  {}{name}: {}",
                    entity.kind.label(),
                    entity.version
                ));
            }
        }
        _ => lines.push("  unavailable".to_string()),
    }
}

fn json_entities(entities: Option<&[FirmwareEntity]>) -> String {
    let Some(entities) = entities else {
        return "null".to_string();
    };
    let entities = entities
        .iter()
        .map(|entity| {
            let name = entity
                .name
                .as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".to_string());
            format!(
                r#"{{"type":"{}","name":{},"version":{}}}"#,
                entity.kind.as_str(),
                name,
                json_string(&entity.version)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entities}]")
}

fn json_string(value: &str) -> String {
    // The crate intentionally has no serialization dependency; these short strings only need the
    // standard JSON escapes handled below.
    let mut escaped = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::{
        FirmwareEntity, FirmwareKind, FirmwareStatus, format_status, parse_mouse_entity,
        parse_receiver_entity,
    };

    #[test]
    fn parses_mouse_firmware_entity() {
        let reply = [
            0x11, 0, 2, 0x11, 0, b'R', b'B', b'M', 0x27, 0x03, 0x00, 0x19,
        ];
        assert_eq!(
            parse_mouse_entity(&reply).unwrap(),
            FirmwareEntity {
                kind: FirmwareKind::Firmware,
                name: Some("RBM".to_string()),
                version: "27.03.B0019".to_string(),
            }
        );
    }

    #[test]
    fn parses_mouse_hardware_entity() {
        let reply = [0x11, 0, 2, 0x11, 2, 72, 0];
        assert_eq!(
            parse_mouse_entity(&reply).unwrap(),
            FirmwareEntity {
                kind: FirmwareKind::Hardware,
                name: None,
                version: "72".to_string(),
            }
        );
    }

    #[test]
    fn parses_bolt_receiver_entity() {
        let reply = [0x11, 0xff, 0x83, 0xf4, 1, 0x30, 0x01, 0x00, 0x10];
        assert_eq!(
            parse_receiver_entity(&reply).unwrap(),
            Some(FirmwareEntity {
                kind: FirmwareKind::Bootloader,
                name: Some("BOT".to_string()),
                version: "30.01.B0010".to_string(),
            })
        );
    }

    #[test]
    fn formats_text_and_json() {
        let status = FirmwareStatus {
            mouse: Some(vec![FirmwareEntity {
                kind: FirmwareKind::Firmware,
                name: Some("RBM".to_string()),
                version: "27.03.B0019".to_string(),
            }]),
            receiver: None,
        };
        assert_eq!(
            format_status(&status, false),
            "Mouse:\n  Firmware RBM: 27.03.B0019\nBolt receiver:\n  unavailable"
        );
        assert_eq!(
            format_status(&status, true),
            r#"{"mouse":[{"type":"firmware","name":"RBM","version":"27.03.B0019"}],"receiver":null}"#
        );
    }
}
