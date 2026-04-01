use hidapi::HidDevice;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use crate::Result;
use crate::device::{SHORT, feature, open, req, write_packet};

const HAPTIC: u16 = 0x19b0;
const HAPTIC_SW_ID: u8 = 0x0e;
const HAPTIC_EFFECT_METHOD: u8 = 0x04;
const HAPTIC_STRENGTH_METHOD: u8 = 0x02;

pub struct HapticStatus {
    pub enabled: bool,
    pub level: u8,
    pub discrete_levels: bool,
}

pub fn play(args: &[String]) -> Result<()> {
    let effects = if args.is_empty() {
        vec![prompt()?]
    } else {
        parse_effects(args)?
    };
    let (dev, idx) = open()?;

    for (i, effect) in effects.iter().copied().enumerate() {
        send_haptic(&dev, idx, effect)?;

        if i + 1 != effects.len() {
            thread::sleep(Duration::from_secs(1));
        }
    }

    Ok(())
}

pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 status haptic --json` if you want JSON".into()),
    };

    match read_status() {
        Ok(status) => println!("{}", format_status(&status, json)),
        Err(_) if json => println!("null"),
        Err(_) => println!("Haptic: unavailable"),
    }
    Ok(())
}

pub fn set_strength_arg(arg: &str) -> Result<()> {
    let (dev, idx) = open()?;
    set_strength(&dev, idx, parse_strength(arg)?)
}

pub fn parse_effects(args: &[String]) -> Result<Vec<u8>> {
    let mut effects = Vec::new();

    for arg in args {
        effects.extend(parse_effect_arg(arg)?);
    }

    Ok(effects)
}

pub fn parse_effect_arg(arg: &str) -> Result<Vec<u8>> {
    if let Some((start, end)) = parse_range(arg)? {
        let effects = if start <= end {
            (start..=end).collect()
        } else {
            (end..=start).rev().collect()
        };
        return Ok(effects);
    }

    Ok(vec![parse_effect(arg)?])
}

pub fn parse_strength(arg: &str) -> Result<u8> {
    let strength = match arg.trim() {
        "off" => 0,
        "subtle" => 15,
        "low" => 45,
        "medium" => 60,
        "high" => 100,
        other => other.parse()?,
    };

    if strength > 100 {
        return Err("pick a haptic strength from 0 to 100".into());
    }

    Ok(strength)
}

pub fn read_status() -> Result<HapticStatus> {
    let (dev, idx) = open()?;
    let feature_idx = feature(&dev, idx, HAPTIC)?;
    let reply = req(&dev, idx, feature_idx, 0x01, &[])?;
    parse_status_reply(&reply)
}

pub fn set_strength(dev: &HidDevice, idx: u8, strength: u8) -> Result<()> {
    let feature_idx = feature(dev, idx, HAPTIC)?;
    let params = if strength == 0 {
        [0x00, 0x32]
    } else {
        [0x01, strength]
    };

    req(dev, idx, feature_idx, HAPTIC_STRENGTH_METHOD, &params)?;
    Ok(())
}

pub fn send_haptic(dev: &HidDevice, idx: u8, effect: u8) -> Result<()> {
    let feature_idx = feature(dev, idx, HAPTIC)?;
    send_method(dev, idx, feature_idx, HAPTIC_EFFECT_METHOD, &[effect])
}

pub fn parse_status_reply(reply: &[u8]) -> Result<HapticStatus> {
    let enabled = reply
        .get(4)
        .copied()
        .ok_or("the haptic reply was too short")?
        & 0x01
        != 0;
    let raw_level = *reply.get(5).ok_or("the haptic reply was too short")?;
    let flags = *reply.get(6).ok_or("the haptic reply was too short")?;

    Ok(HapticStatus {
        enabled,
        level: if enabled { raw_level } else { 0 },
        discrete_levels: flags & 0x01 != 0,
    })
}

pub fn format_status(status: &HapticStatus, json: bool) -> String {
    if json {
        format!(
            r#"{{"enabled":{},"level":{},"discrete_levels":{}}}"#,
            status.enabled, status.level, status.discrete_levels
        )
    } else if status.enabled {
        format!(
            "Haptic: {}{}",
            status.level,
            if status.discrete_levels {
                " (discrete levels)"
            } else {
                ""
            }
        )
    } else {
        "Haptic: off".to_string()
    }
}

pub fn json_status() -> Result<String> {
    Ok(format_status(&read_status()?, true))
}

pub fn packet(idx: u8, feature_idx: u8, method: u8, params: &[u8]) -> [u8; 7] {
    let mut pkt = [0u8; 7];
    pkt[0] = SHORT;
    pkt[1] = idx;
    pkt[2] = feature_idx;
    pkt[3] = (method << 4) | HAPTIC_SW_ID;
    pkt[4..4 + params.len().min(3)].copy_from_slice(&params[..params.len().min(3)]);
    pkt
}

fn parse_range(arg: &str) -> Result<Option<(u8, u8)>> {
    let trimmed = arg.trim();
    let inner = if trimmed.starts_with('{') && trimmed.ends_with('}') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let Some((start, end)) = inner.split_once("..") else {
        return Ok(None);
    };

    Ok(Some((parse_effect(start)?, parse_effect(end)?)))
}

fn parse_effect(arg: &str) -> Result<u8> {
    let effect: u8 = arg.trim().parse()?;

    if effect > 14 {
        return Err("pick a haptic effect from 0 to 14".into());
    }

    Ok(effect)
}

fn send_method(dev: &HidDevice, idx: u8, feature_idx: u8, method: u8, params: &[u8]) -> Result<()> {
    let pkt = packet(idx, feature_idx, method, params);
    write_packet(dev, &pkt, "that haptic packet didn't fully send")
}

fn prompt() -> Result<u8> {
    loop {
        print!("Enter a number from 0 to 14: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().parse() {
            Ok(n @ 0..=14) => return Ok(n),
            _ => println!("Pick a haptic effect from 0 to 14."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HAPTIC_EFFECT_METHOD, HAPTIC_STRENGTH_METHOD, format_status, packet, parse_effect_arg,
        parse_effects, parse_status_reply, parse_strength,
    };

    #[test]
    fn parses_single_effect() {
        assert_eq!(parse_effect_arg("14").unwrap(), vec![14]);
    }

    #[test]
    fn parses_zero_effect() {
        assert_eq!(parse_effect_arg("0").unwrap(), vec![0]);
    }

    #[test]
    fn parses_plain_range() {
        assert_eq!(parse_effect_arg("1..3").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn parses_braced_range() {
        assert_eq!(parse_effect_arg("{1..3}").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn parses_descending_range() {
        assert_eq!(parse_effect_arg("{3..1}").unwrap(), vec![3, 2, 1]);
    }

    #[test]
    fn combines_multiple_args() {
        let args = vec!["0".to_string(), "{3..4}".to_string(), "2".to_string()];
        assert_eq!(parse_effects(&args).unwrap(), vec![0, 3, 4, 2]);
    }

    #[test]
    fn parses_named_strengths() {
        assert_eq!(parse_strength("off").unwrap(), 0);
        assert_eq!(parse_strength("subtle").unwrap(), 15);
        assert_eq!(parse_strength("low").unwrap(), 45);
        assert_eq!(parse_strength("medium").unwrap(), 60);
        assert_eq!(parse_strength("high").unwrap(), 100);
    }

    #[test]
    fn parses_numeric_strength() {
        assert_eq!(parse_strength("37").unwrap(), 37);
    }

    #[test]
    fn rejects_out_of_range_strength() {
        assert!(parse_strength("101").is_err());
    }

    #[test]
    fn builds_effect_packet() {
        assert_eq!(
            packet(7, 0x0b, HAPTIC_EFFECT_METHOD, &[3]),
            [0x10, 7, 0x0b, 0x4e, 3, 0, 0]
        );
    }

    #[test]
    fn builds_strength_packet() {
        assert_eq!(
            packet(7, 0x0b, HAPTIC_STRENGTH_METHOD, &[0x01, 60]),
            [0x10, 7, 0x0b, 0x2e, 0x01, 60, 0]
        );
    }

    #[test]
    fn parses_enabled_haptic_status() {
        let status = parse_status_reply(&[0, 0, 0, 0, 0x01, 60, 0x00]).unwrap();
        assert!(status.enabled);
        assert_eq!(status.level, 60);
        assert!(!status.discrete_levels);
    }

    #[test]
    fn parses_disabled_haptic_status() {
        let status = parse_status_reply(&[0, 0, 0, 0, 0x00, 99, 0x01]).unwrap();
        assert!(!status.enabled);
        assert_eq!(status.level, 0);
        assert!(status.discrete_levels);
    }

    #[test]
    fn formats_disabled_haptic_status() {
        let status = parse_status_reply(&[0, 0, 0, 0, 0x00, 99, 0x01]).unwrap();
        assert_eq!(format_status(&status, false), "Haptic: off");
    }

    #[test]
    fn formats_enabled_haptic_json() {
        let status = parse_status_reply(&[0, 0, 0, 0, 0x01, 60, 0x01]).unwrap();
        assert_eq!(
            format_status(&status, true),
            r#"{"enabled":true,"level":60,"discrete_levels":true}"#
        );
    }
}
