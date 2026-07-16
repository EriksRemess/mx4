//! Command-line parsing and presentation for the `mx4` binary.

use mx4::{Result, autostart, config, daemon, features};

const HELP_SYNTAX_WIDTH: usize = 34;

const HELP_COMMANDS: &[(&str, &str)] = &[
    ("battery [--json]", "Show battery level and charging state"),
    (
        "daemon [--once]",
        "Reapply saved settings continuously or once",
    ),
    ("firmware [--json]", "Show mouse and Bolt receiver firmware"),
    (
        "haptic [<effect|range>...]",
        "Play haptic effects from 0 to 14",
    ),
    (
        "set <target> ...",
        "Change a setting; most settings are saved",
    ),
    (
        "status [<target>] [--json]",
        "Show all status values or one target",
    ),
];

const HELP_SET_TARGETS: &[(&str, &str)] = &[
    ("dpi <200-8000>", "Set sensor resolution in DPI"),
    (
        "force-button <value>",
        "Set the force-sensing button threshold",
    ),
    ("host <1|2|3>", "Switch the active Easy-Switch host"),
    (
        "strength <0-100|preset>",
        "Set haptic strength or turn it off",
    ),
    (
        "thumb-wheel divert <on|off>",
        "Route thumb-wheel events through HID++",
    ),
    (
        "thumb-wheel invert <on|off>",
        "Reverse the thumb-wheel direction",
    ),
    (
        "wheel divert <on|off>",
        "Route scroll-wheel events through HID++",
    ),
    ("wheel force <1-100>", "Set ratchet resistance"),
    (
        "wheel invert <on|off>",
        "Reverse the scroll-wheel direction",
    ),
    (
        "wheel ratchet <free|ratchet>",
        "Select free-spin or ratchet mode",
    ),
    (
        "wheel ratchet-speed <0-50>",
        "Set the SmartShift transition speed",
    ),
    (
        "wheel resolution <on|off>",
        "Toggle high-resolution scrolling",
    ),
];

const HELP_STATUS_TARGETS: &[(&str, &str)] = &[
    ("battery", "Battery level and charging state"),
    ("dpi", "Current sensor resolution"),
    ("force-button", "Current threshold and supported range"),
    ("haptic", "Haptic state and strength"),
    ("thumb-wheel", "Thumb-wheel direction and diversion state"),
    ("wheel", "Ratchet, SmartShift, and scroll-wheel state"),
];

const HELP_OPTIONS: &[(&str, &str)] = &[
    ("-h | --help", "Show this help"),
    ("--json", "Print machine-readable JSON where supported"),
    ("-v | --version", "Show mx4 and hidapi versions"),
];

fn print_help() {
    println!("mx4 - MX Master 4 status and settings");
    println!();
    println!("Usage:");
    println!("  mx4 <command> [arguments]");
    println!();
    print_help_section("Commands:", HELP_COMMANDS);
    print_help_section("Set targets:", HELP_SET_TARGETS);
    print_help_section("Status targets (all accept --json):", HELP_STATUS_TARGETS);
    print_help_section("Options:", HELP_OPTIONS);
    println!("Haptic strength presets: off, subtle, low, medium, high");
    println!("Haptic ranges: 0..14 or {{0..14}}");
}

fn print_help_section(title: &str, rows: &[(&str, &str)]) {
    println!("{title}");
    for (syntax, description) in rows {
        // One shared width keeps every description aligned, even across separate help sections.
        println!(
            "  {syntax:<width$}  {description}",
            width = HELP_SYNTAX_WIDTH
        );
    }
    println!();
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let wants_help_or_version = matches!(
        args.first().map(String::as_str),
        None | Some("-h" | "--help" | "-v" | "--version")
    );
    // Installing the reconnect daemon is a side effect, so informational commands remain safe to
    // run during packaging, completion, and diagnostics.
    if !wants_help_or_version {
        if let Err(err) = autostart::ensure_installed() {
            eprintln!("Warning: couldn't install background daemon: {err}");
        }
    }

    let mut args = args.into_iter();

    match args.next().as_deref() {
        None | Some("-h" | "--help") => {
            print_help();
        }
        Some("-v" | "--version") => println!("{}", version_output()),
        Some("status") => status(args.collect())?,
        Some("set") => set(args.collect())?,
        Some("daemon") => daemon::run(&args.collect::<Vec<_>>())?,
        Some("haptic") => haptic(args.collect())?,
        Some("battery") => features::battery::status(args.next().as_deref())?,
        Some("firmware") => features::firmware::status(args.next().as_deref())?,
        Some(_) => {
            return Err(
                "I only know `status`, `set`, `daemon`, `haptic`, `battery`, and `firmware` right now".into(),
            );
        }
    }

    Ok(())
}

fn version_output() -> String {
    format!(
        "mx4 {}\nhidapi {}",
        env!("CARGO_PKG_VERSION"),
        option_env!("MX4_HIDAPI_VERSION").unwrap_or("unknown")
    )
}

fn status(args: Vec<String>) -> Result<()> {
    match args.as_slice() {
        [] => {
            features::battery::print_best_effort();
            features::dpi::status(None)?;
            features::wheel::status(None)?;
            features::wheel::thumb_status(None)?;
            features::force_button::status(None)?;
            features::haptic::status(None)?;
            Ok(())
        }
        [flag] if flag == "--json" => {
            println!("{}", json_status());
            Ok(())
        }
        [target] if target == "battery" => features::battery::status(None),
        [target] if target == "dpi" => features::dpi::status(None),
        [target] if target == "haptic" => features::haptic::status(None),
        [target] if target == "wheel" => features::wheel::status(None),
        [target] if target == "thumb-wheel" => features::wheel::thumb_status(None),
        [target] if target == "force-button" => features::force_button::status(None),
        [target, flag] if target == "battery" => features::battery::status(Some(flag.as_str())),
        [target, flag] if target == "dpi" => features::dpi::status(Some(flag.as_str())),
        [target, flag] if target == "haptic" => features::haptic::status(Some(flag.as_str())),
        [target, flag] if target == "wheel" => features::wheel::status(Some(flag.as_str())),
        [target, flag] if target == "thumb-wheel" => features::wheel::thumb_status(Some(flag.as_str())),
        [target, flag] if target == "force-button" => features::force_button::status(Some(flag.as_str())),
        _ => Err(
            "try `mx4 status`, `mx4 status --json`, `mx4 status battery --json`, `mx4 status dpi --json`, `mx4 status wheel --json`, `mx4 status thumb-wheel --json`, `mx4 status force-button --json`, or `mx4 status haptic --json`".into(),
        ),
    }
}

fn json_status() -> String {
    // Status is intentionally best-effort: unsupported features remain visible as `null` instead of
    // making the entire machine-readable snapshot fail.
    format!(
        r#"{{"battery":{},"dpi":{},"wheel":{},"thumb_wheel":{},"force_button":{},"haptic":{}}}"#,
        best_effort_json(features::battery::json_status()),
        best_effort_json(features::dpi::json_status()),
        best_effort_json(features::wheel::json_status()),
        best_effort_json(features::wheel::json_thumb_status()),
        best_effort_json(features::force_button::json_status()),
        best_effort_json(features::haptic::json_status()),
    )
}

fn best_effort_json(result: Result<String>) -> String {
    result.unwrap_or_else(|_| "null".to_string())
}

fn set(args: Vec<String>) -> Result<()> {
    // Each helper applies the live device change before updating the saved reconnect configuration.
    // Invalid or unsupported values therefore never become persistent desired state.
    match args.as_slice() {
        [target, value] if target == "strength" => set_strength(value),
        [target, value] if target == "host" => features::host::set(value),
        [target, value] if target == "dpi" => set_dpi(value),
        [target, setting, value] if target == "wheel" => set_wheel(setting, value),
        [target, setting, value] if target == "thumb-wheel" => {
            set_thumb_wheel(setting, value)
        }
        [target, value] if target == "force-button" => set_force_button(value),
        _ => Err(
            "try `mx4 set strength 100`, `mx4 set host 2`, `mx4 set dpi 2500`, `mx4 set wheel ratchet free`, `mx4 set wheel ratchet-speed 10`, or `mx4 set thumb-wheel invert on`"
                .into(),
        ),
    }
}

fn haptic(args: Vec<String>) -> Result<()> {
    if let [mode, _value] = args.as_slice() {
        if mode == "strength" {
            return Err("use `mx4 set strength ...` instead of `mx4 haptic strength ...`".into());
        }
    }

    features::haptic::play(&args)
}

fn set_strength(value: &str) -> Result<()> {
    let strength = features::haptic::parse_strength(value)?;
    features::haptic::set_strength_arg(value)?;
    config::update(|saved| saved.haptic_strength = Some(strength))
}

fn set_dpi(value: &str) -> Result<()> {
    let dpi = features::dpi::parse(value)?;
    features::dpi::set(value)?;
    config::update(|saved| saved.dpi = Some(dpi))
}

fn set_wheel(setting: &str, value: &str) -> Result<()> {
    features::wheel::set(setting, value)?;

    match setting {
        "ratchet" => {
            let ratchet = parse_ratchet(value)?;
            config::update(|saved| saved.wheel_ratchet = Some(ratchet))
        }
        "ratchet-speed" | "smart-shift" => {
            let speed: u8 = value.trim().parse()?;
            config::update(|saved| saved.wheel_ratchet_speed = Some(speed))
        }
        "force" => {
            let force: u8 = value.trim().parse()?;
            config::update(|saved| saved.wheel_force = Some(force))
        }
        "invert" => {
            let enabled = parse_toggle(value)?;
            config::update(|saved| saved.wheel_invert = Some(enabled))
        }
        "resolution" => {
            let enabled = parse_toggle(value)?;
            config::update(|saved| saved.wheel_resolution = Some(enabled))
        }
        "divert" => {
            let enabled = parse_toggle(value)?;
            config::update(|saved| saved.wheel_divert = Some(enabled))
        }
        _ => Ok(()),
    }
}

fn set_thumb_wheel(setting: &str, value: &str) -> Result<()> {
    features::wheel::set_thumb(setting, value)?;
    let enabled = parse_toggle(value)?;

    match setting {
        "invert" => config::update(|saved| saved.thumb_wheel_invert = Some(enabled)),
        "divert" => config::update(|saved| saved.thumb_wheel_divert = Some(enabled)),
        _ => Ok(()),
    }
}

fn set_force_button(value: &str) -> Result<()> {
    let force_button: u16 = value.trim().parse()?;
    features::force_button::set(value)?;
    config::update(|saved| saved.force_button = Some(force_button))
}

fn parse_toggle(value: &str) -> Result<bool> {
    match value.trim() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("pick `on` or `off`".into()),
    }
}

fn parse_ratchet(value: &str) -> Result<config::WheelRatchet> {
    match value.trim() {
        "free" => Ok(config::WheelRatchet::Free),
        "ratchet" => Ok(config::WheelRatchet::Ratchet),
        _ => Err("pick `free` or `ratchet`".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HELP_COMMANDS, HELP_OPTIONS, HELP_SET_TARGETS, HELP_STATUS_TARGETS, version_output,
    };

    #[test]
    fn help_explains_commands_targets_and_options() {
        assert!(
            HELP_COMMANDS
                .iter()
                .any(|(syntax, _)| syntax.starts_with("firmware "))
        );
        assert!(!HELP_SET_TARGETS.is_empty());
        assert!(!HELP_STATUS_TARGETS.is_empty());
        assert!(
            HELP_OPTIONS
                .iter()
                .any(|(syntax, _)| *syntax == "-h | --help")
        );
    }

    #[test]
    fn version_output_includes_package_and_hidapi_versions() {
        let output = version_output();
        assert!(output.contains(env!("CARGO_PKG_VERSION")));
        assert!(output.contains("hidapi "));
    }
}
