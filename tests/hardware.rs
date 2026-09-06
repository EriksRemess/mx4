//! Opt-in checks against a connected, awake MX Master 4.
//! Run with `cargo test hardware -- --ignored --nocapture`.

use std::process::{Command, Output, Stdio};

use mx4::features::{battery, dpi, firmware, force_button, haptic, wheel};

#[test]
#[ignore = "requires a connected MX Master 4; sends haptic effect 14"]
fn hardware_haptic_playback() {
    let before = haptic::json_status().expect("read haptic configuration before playback");
    println!("Sending haptic effect 14; feel the mouse to confirm physical feedback.");
    let output = Command::new(env!("CARGO_BIN_EXE_mx4"))
        .args(["haptic", "14"])
        .output()
        .expect("run haptic playback command");
    assert!(
        output.status.success(),
        "haptic playback command failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        haptic::json_status().expect("read haptic configuration after playback"),
        before,
        "effect playback changed the haptic configuration"
    );
    // Playback has no acknowledgement: successful HID writes cannot prove physical vibration.
    println!("Haptic command sent successfully; configuration unchanged.");
}

#[test]
#[ignore = "requires a connected MX Master 4 and HID access"]
fn hardware_status() {
    let battery = battery::read_status().expect("read battery");
    assert!(battery.pct <= 100, "invalid battery percentage");
    println!("{}", battery::format_status(&battery, false));

    let dpi = dpi::read_status().expect("read DPI");
    dpi::parse(&dpi.to_string()).expect("DPI is within the supported range");
    println!("{}", dpi::format_status(dpi, false));

    let (smart_shift, hires) = wheel::read_status().expect("read wheel");
    println!("{}", wheel::format_status(&smart_shift, &hires, false));
    let thumb = wheel::read_thumb_status().expect("read thumb wheel");
    println!("{}", wheel::format_thumb_status(&thumb, false));

    let (force, info) = force_button::read_status().expect("read force button");
    assert!(
        (info.min_value..=info.max_value).contains(&force),
        "force-button value is outside the reported range"
    );
    println!("{}", force_button::format_status(force, &info, false));

    let haptic = haptic::read_status().expect("read haptic configuration");
    assert!(haptic.level <= 100, "invalid haptic level");
    println!("{}", haptic::format_status(&haptic, false));

    // Read each side directly: the aggregate firmware API tolerates an unavailable mouse.
    let mouse = firmware::read_mouse().expect("read mouse firmware");
    assert!(!mouse.is_empty(), "no mouse firmware entities");
    let receiver = firmware::read_receiver().expect("read receiver firmware when present");
    if let Some(entities) = &receiver {
        assert!(!entities.is_empty(), "no receiver firmware entities");
    }
    println!(
        "{}",
        firmware::format_status(
            &firmware::FirmwareStatus {
                mouse: Some(mouse),
                receiver,
            },
            false,
        )
    );
}

fn query(feature: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx4"));
    command
        .args(["status", feature, "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn successful_output(feature: &str, output: Output) -> String {
    assert!(
        output.status.success(),
        "{feature} query failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 status output")
}

#[test]
#[ignore = "requires a connected MX Master 4; leave settings unchanged during the test"]
fn hardware_concurrent_queries() {
    let expected_dpi = format!("{}\n", dpi::json_status().expect("read baseline DPI"));
    let expected_haptic = format!("{}\n", haptic::json_status().expect("read baseline haptic"));
    for round in 1..=10 {
        // Different feature lookups used to accept each other's replies across processes.
        let dpi = query("dpi").spawn().expect("start DPI query");
        let haptic = query("haptic").spawn().expect("start haptic query");
        let dpi = dpi.wait_with_output().expect("wait for DPI query");
        let haptic = haptic.wait_with_output().expect("wait for haptic query");
        assert_eq!(successful_output("dpi", dpi), expected_dpi, "round {round}");
        assert_eq!(
            successful_output("haptic", haptic),
            expected_haptic,
            "round {round}"
        );
    }
    println!("10 concurrent DPI/haptic query pairs passed");
}
