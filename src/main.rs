use hidapi::HidApi;
use std::io::{self, Write};

const VID: u16 = 0x046d;
const PID: u16 = 0xc548;
const USAGE_PAGE: u16 = 0xff00;
const REPORT_ID: u8 = 0x10;
const FEATURE_HAPTIC: u16 = 0x0b4e;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    if matches!(command.as_deref(), None | Some("-h" | "--help")) {
        println!("Usage: mx4 haptic <1-15>");
        println!("Try: mx4 haptic 4");
        return Ok(());
    }

    match command.as_deref() {
        Some("haptic") => {
            let effect = match args.next() {
                Some(arg) => arg.parse()?,
                None => prompt_for_effect()?,
            };
            haptic(effect)?;
            Ok(())
        }
        Some(_) => Err("I only know the `haptic` command right now".into()),
        None => Ok(()),
    }
}

fn haptic(effect: u8) -> Result<(), Box<dyn std::error::Error>> {
    if !(1..=15).contains(&effect) {
        return Err("pick a haptic effect from 1 to 15".into());
    }

    let api = HidApi::new()?;

    let device = api
        .device_list()
        .find(|device| {
            device.vendor_id() == VID
                && device.product_id() == PID
                && device.usage_page() == USAGE_PAGE
        })
        .ok_or("couldn't find your MX receiver")?;

    let interface = u8::try_from(device.interface_number())?;
    let feature = FEATURE_HAPTIC.to_be_bytes();
    let packet = [
        REPORT_ID,
        interface,
        feature[0],
        feature[1],
        effect - 1,
        0,
        0,
    ];

    device.open_device(&api)?.write(&packet)?;
    Ok(())
}

fn prompt_for_effect() -> Result<u8, Box<dyn std::error::Error>> {
    loop {
        print!("Enter a number from 1 to 15: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().parse::<u8>() {
            Ok(effect) if (1..=15).contains(&effect) => return Ok(effect),
            _ => println!("Pick a haptic effect from 1 to 15."),
        }
    }
}
