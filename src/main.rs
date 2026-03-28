use hidapi::{HidApi, HidDevice};
use std::io::{self, Write};

const VID: u16 = 0x046d;
const PID: u16 = 0xc548;
const PAGE: u16 = 0xff00;

const SHORT: u8 = 0x10;
const LONG: u8 = 0x11;
const SW_ID: u8 = 0x01;

const HAPTIC: u16 = 0x0b4e;
const BATTERY: u16 = 0x1000;
const UNIFIED_BATTERY: u16 = 0x1004;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        None | Some("-h" | "--help") => {
            println!("Usage:");
            println!("  mx4 haptic <1-15>");
            println!("  mx4 battery [--json]");
        }
        Some("haptic") => haptic(args.next())?,
        Some("battery") => battery(args.next().as_deref())?,
        Some(_) => return Err("I only know `haptic` and `battery` right now".into()),
    }

    Ok(())
}

fn haptic(arg: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let effect = match arg {
        Some(arg) => arg.parse()?,
        None => prompt()?,
    };

    if !(1..=15).contains(&effect) {
        return Err("pick a haptic effect from 1 to 15".into());
    }

    let (dev, idx) = open()?;
    let f = HAPTIC.to_be_bytes();
    let pkt = [SHORT, idx, f[0], f[1], effect - 1, 0, 0];

    if dev.write(&pkt)? != pkt.len() {
        return Err("that haptic packet didn't fully send".into());
    }

    Ok(())
}

fn battery(arg: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let json = match arg {
        None => false,
        Some("--json") => true,
        Some(_) => return Err("try `mx4 battery --json` if you want JSON".into()),
    };

    let (dev, idx) = open()?;
    let (feature, unified) = match feature(&dev, idx, UNIFIED_BATTERY) {
        Ok(feature) => (feature, true),
        Err(_) => (feature(&dev, idx, BATTERY)?, false),
    };
    let reply = req(&dev, idx, feature, if unified { 1 } else { 0 }, &[])?;
    let pct = *reply.get(4).ok_or("the battery reply was too short")?;
    let charging = if unified {
        matches!(reply.get(7).copied(), Some(1..=3))
    } else {
        matches!(reply.get(6).copied(), Some(1..=4))
    };

    if json {
        println!(r#"{{"battery":{},"charging":{}}}"#, pct, charging);
    } else {
        println!(
            "Battery: {}%{}",
            pct,
            if charging { " (charging)" } else { "" }
        );
    }

    Ok(())
}

fn open() -> Result<(HidDevice, u8), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == PAGE)
        .ok_or("couldn't find your MX receiver")?;

    Ok((info.open_device(&api)?, u8::try_from(info.interface_number())?))
}

fn feature(
    dev: &HidDevice,
    idx: u8,
    feature_id: u16,
) -> Result<u8, Box<dyn std::error::Error>> {
    let reply = req(dev, idx, 0x00, 0x00, &feature_id.to_be_bytes())?;
    let feature = *reply.get(4).ok_or("the feature lookup reply was too short")?;

    if feature == 0 {
        return Err("that feature isn't available on this device".into());
    }

    Ok(feature)
}

fn req(
    dev: &HidDevice,
    idx: u8,
    feature: u8,
    function: u8,
    params: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut drain = [0u8; 64];
    while dev.read_timeout(&mut drain, 0)? != 0 {}

    let mut pkt = [0u8; 7];
    pkt[0] = SHORT;
    pkt[1] = idx;
    pkt[2] = feature;
    pkt[3] = (function << 4) | SW_ID;
    pkt[4..4 + params.len().min(3)].copy_from_slice(&params[..params.len().min(3)]);

    if dev.write(&pkt)? != pkt.len() {
        return Err("that request didn't fully send".into());
    }

    let mut reply = [0u8; 20];

    for _ in 0..100 {
        let len = dev.read_timeout(&mut reply, 10)?;
        if len < 7 {
            continue;
        }

        if (reply[0] == SHORT || reply[0] == LONG)
            && reply[1] == idx
            && reply[2] == feature
            && reply[3] >> 4 == function
            && reply[3] & 0x0f == SW_ID
        {
            return Ok(reply[..len].to_vec());
        }
    }

    Err("the device didn't answer in time".into())
}

fn prompt() -> Result<u8, Box<dyn std::error::Error>> {
    loop {
        print!("Enter a number from 1 to 15: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().parse() {
            Ok(n @ 1..=15) => return Ok(n),
            _ => println!("Pick a haptic effect from 1 to 15."),
        }
    }
}
