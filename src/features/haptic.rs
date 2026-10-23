//! Haptic status, strength, and effect playback.

use hidapi::HidDevice;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use crate::Result;
use crate::device;

const HAPTIC: u16 = 0x19b0;
const HAPTIC_SW_ID: u8 = 0x0e;
const HAPTIC_EFFECT_METHOD: u8 = 0x04;
const HAPTIC_STRENGTH_METHOD: u8 = 0x02;

/// Haptic enable state, effective level, and whether the device reports discrete levels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HapticStatus {
    pub enabled: bool,
    pub level: u8,
    pub discrete_levels: bool,
}

/// Play effect numbers or ranges in order, pausing one second between effects.
///
/// With no arguments, prompts on standard input and output. Success confirms packet writes, not
/// physical vibration. Playback does not change saved settings.
pub fn play(args: &[String]) -> Result<()> {
    let effects = if args.is_empty() {
        vec![prompt()?]
    } else {
        parse_effects(args)?
    };
    let (dev, idx) = device::open()?;

    // Leave enough time for each physical effect to finish before triggering the next one.
    for (i, effect) in effects.iter().copied().enumerate() {
        send_haptic(&dev, idx, effect)?;

        if i + 1 != effects.len() {
            thread::sleep(Duration::from_secs(1));
        }
    }

    Ok(())
}

/// Read and print status; `arg` accepts `None` for text or `Some("--json")` for JSON.
pub fn status(arg: Option<&str>) -> Result<()> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 status haptic --json` if you want JSON".into()),
    };

    let status = read_status()?;
    println!("{}", format_status(&status, json));
    Ok(())
}

/// Print text status, or an unavailable message if reading the device fails.
pub fn print_best_effort() {
    match read_status() {
        Ok(status) => println!("{}", format_status(&status, false)),
        Err(_) => println!("Haptic: unavailable"),
    }
}

/// Parse and apply a strength or preset without saving it to configuration.
pub fn set_strength_arg(arg: &str) -> Result<()> {
    let (dev, idx) = device::open()?;
    set_strength(&dev, idx, parse_strength(arg)?)
}

/// Expand effect arguments in order, preserving duplicates and each range's direction.
pub fn parse_effects(args: &[String]) -> Result<Vec<u8>> {
    let mut effects = Vec::new();

    for arg in args {
        effects.extend(parse_effect_arg(arg)?);
    }

    Ok(effects)
}

/// Parse an effect from 0 through 14 or an inclusive range such as `1..3` or `{3..1}`.
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

/// Parse a strength from 0 through 100 or the presets `off`, `subtle`, `low`, `medium`, `high`.
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

/// Read haptic settings, reporting a level of zero when haptics are disabled.
pub fn read_status() -> Result<HapticStatus> {
    let (dev, idx) = device::open()?;
    let feature_idx = device::feature(&dev, idx, HAPTIC)?;
    let reply = device::req(&dev, idx, feature_idx, 0x01, &[])?;
    parse_status_reply(&reply)
}

/// Apply haptic strength using an existing device handle and device index.
///
/// Zero disables haptics. The caller must validate the 0 through 100 range, for example with
/// [`parse_strength`]; this low-level setter does not validate it or persist the setting.
pub fn set_strength(dev: &HidDevice, idx: u8, strength: u8) -> Result<()> {
    let feature_idx = device::feature(dev, idx, HAPTIC)?;
    // The first byte is an enable flag. Disabled mode still carries a placeholder level, while an
    // enabled request carries the caller's actual strength.
    let params = if strength == 0 {
        [0x00, 0x32]
    } else {
        [0x01, strength]
    };

    device::req(dev, idx, feature_idx, HAPTIC_STRENGTH_METHOD, &params)?;
    Ok(())
}

/// Send an effect using an existing device handle and device index, without awaiting a reply.
///
/// The caller must validate the effect number (0 through 14), for example with [`parse_effect_arg`].
/// Success confirms the packet write only; it does not confirm physical vibration.
pub fn send_haptic(dev: &HidDevice, idx: u8, effect: u8) -> Result<()> {
    let feature_idx = device::feature(dev, idx, HAPTIC)?;
    send_method(dev, idx, feature_idx, HAPTIC_EFFECT_METHOD, &[effect])
}

/// Decode a complete HID++ status report, returning an error if it is too short.
///
/// Disabled haptics are normalized to level zero regardless of the reported placeholder level.
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

/// Format status without device I/O; `json` selects JSON (`true`) or text (`false`).
#[must_use]
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

/// Read status and return JSON without printing it.
pub fn json_status() -> Result<String> {
    Ok(format_status(&read_status()?, true))
}

/// Build a short haptic report with the playback software ID, without device I/O.
///
/// Copies at most three parameter bytes and uses the low four bits of `method`. This low-level
/// helper truncates excess data rather than validating it.
#[must_use]
pub fn packet(idx: u8, feature_idx: u8, method: u8, params: &[u8]) -> [u8; 7] {
    // Effect playback is fire-and-forget, so build the packet with a dedicated software ID and send
    // it directly instead of waiting for the generic request/reply matcher.
    let mut pkt = [0u8; 7];
    pkt[0] = device::SHORT;
    pkt[1] = idx;
    pkt[2] = feature_idx;
    pkt[3] = (method << 4) | HAPTIC_SW_ID;
    pkt[4..4 + params.len().min(3)].copy_from_slice(&params[..params.len().min(3)]);
    pkt
}

fn parse_range(arg: &str) -> Result<Option<(u8, u8)>> {
    let trimmed = arg.trim();
    // Shell brace expansion may turn `{0..3}` into separate arguments before mx4 sees it. Accept the
    // braced form too for quoted input and tests, alongside the plain `0..3` form.
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
    let _lock = crate::transport_lock::acquire()?;
    let pkt = packet(idx, feature_idx, method, params);
    device::write_packet(dev, &pkt, "that haptic packet didn't fully send")
}

fn prompt() -> Result<u8> {
    prompt_with(&mut io::stdin().lock(), &mut io::stdout().lock())
}

fn prompt_with(input: &mut impl io::BufRead, output: &mut impl Write) -> Result<u8> {
    loop {
        write!(output, "Enter a number from 0 to 14: ")?;
        output.flush()?;

        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Err("no haptic effect supplied before end of input".into());
        }

        match line.trim().parse() {
            Ok(n @ 0..=14) => return Ok(n),
            _ => writeln!(output, "Pick a haptic effect from 0 to 14.")?,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prompt_handles_eof_and_invalid_input() {
        let mut output = Vec::new();
        assert!(super::prompt_with(&mut &b""[..], &mut output).is_err());
        assert!(output.len() < 100);
        assert!(super::prompt_with(&mut &b"invalid\n"[..], &mut output).is_err());
        assert_eq!(
            super::prompt_with(&mut &b"invalid\n3\n"[..], &mut output).unwrap(),
            3
        );
    }
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
