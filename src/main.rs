use hidapi::HidApi;

const VID: u16 = 0x046d;
const PID: u16 = 0xc548;
const USAGE_PAGE: u16 = 0xff00;
const REPORT_ID: u8 = 0x10;
const FEATURE_HAPTIC: u16 = 0x0b4e;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let effect = std::env::args()
        .nth(1)
        .map(|arg| arg.parse())
        .transpose()?
        .unwrap_or(1);

    vibrate(effect)?;
    Ok(())
}

fn vibrate(effect: u8) -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    let device = api
        .device_list()
        .find(|device| {
            device.vendor_id() == VID
                && device.product_id() == PID
                && device.usage_page() == USAGE_PAGE
        })
        .ok_or("device not found")?;

    let interface = u8::try_from(device.interface_number())?;
    let feature = FEATURE_HAPTIC.to_be_bytes();
    let packet = [REPORT_ID, interface, feature[0], feature[1], effect, 0, 0];

    device.open_device(&api)?.write(&packet)?;
    Ok(())
}
