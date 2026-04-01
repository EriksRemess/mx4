use mx4::{Result, features};

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);

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
            println!("  mx4 haptic <0-14|0..14|{{0..14}}>...");
            println!("  mx4 battery [--json]");
        }
        Some("status") => status(args.collect())?,
        Some("set") => set(args.collect())?,
        Some("haptic") => haptic(args.collect())?,
        Some("battery") => features::battery::status(args.next().as_deref())?,
        Some(_) => {
            return Err("I only know `status`, `set`, `haptic`, and `battery` right now".into());
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
        [target, value] if target == "strength" => features::haptic::set_strength_arg(value),
        [target, value] if target == "host" => features::host::set(value),
        [target, value] if target == "dpi" => features::dpi::set(value),
        [target, setting, value] if target == "wheel" => features::wheel::set(setting, value),
        [target, setting, value] if target == "thumb-wheel" => {
            features::wheel::set_thumb(setting, value)
        }
        [target, value] if target == "force-button" => features::force_button::set(value),
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
