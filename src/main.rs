use mx4::{Result, autostart, config, daemon, features};

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let wants_help = matches!(
        args.first().map(String::as_str),
        None | Some("-h" | "--help")
    );
    if !wants_help {
        if let Err(err) = autostart::ensure_installed() {
            eprintln!("Warning: couldn't install background daemon: {err}");
        }
    }

    let mut args = args.into_iter();

    match args.next().as_deref() {
        None | Some("-h" | "--help") => {
            println!("Usage:");
            println!("  mx4 status [--json]");
            println!("  mx4 status battery [--json]");
            println!("  mx4 status dpi [--json]");
            println!("  mx4 status haptic [--json]");
            println!("  mx4 status wheel [--json]");
            println!("  mx4 status thumb-wheel [--json]");
            println!("  mx4 status force-button [--json]");
            println!("  mx4 set strength <0-100|off|subtle|low|medium|high>");
            println!("  mx4 set host <1|2|3>");
            println!("  mx4 set dpi <value>");
            println!("  mx4 set wheel ratchet <free|ratchet>");
            println!("  mx4 set wheel ratchet-speed <0-50>");
            println!("  mx4 set wheel force <1-100>");
            println!("  mx4 set wheel invert <on|off>");
            println!("  mx4 set wheel resolution <on|off>");
            println!("  mx4 set wheel divert <on|off>");
            println!("  mx4 set thumb-wheel invert <on|off>");
            println!("  mx4 set thumb-wheel divert <on|off>");
            println!("  mx4 set force-button <value>");
            println!("  mx4 daemon [--once]");
            println!("  mx4 haptic <0-14|0..14|{{0..14}}>...");
            println!("  mx4 battery [--json]");
        }
        Some("status") => status(args.collect())?,
        Some("set") => set(args.collect())?,
        Some("daemon") => daemon::run(&args.collect::<Vec<_>>())?,
        Some("haptic") => haptic(args.collect())?,
        Some("battery") => features::battery::status(args.next().as_deref())?,
        Some(_) => {
            return Err(
                "I only know `status`, `set`, `daemon`, `haptic`, and `battery` right now".into(),
            );
        }
    }

    Ok(())
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
