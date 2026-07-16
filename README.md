# mx4

Tiny CLI for MX Master 4 status and settings.

## Installation

```bash
cargo install mx4
```

### Requirements

Cargo builds mx4 from source. It requires a current stable Rust toolchain, a C build toolchain, `pkg-config`, and the libudev development files on Linux. On Debian or Ubuntu, install the native build dependencies with:

```bash
sudo apt install build-essential pkg-config libudev-dev
```

Cargo places the binary in `~/.cargo/bin`. Make sure that directory is included in `PATH`.

### Install from source

```bash
git clone https://github.com/EriksRemess/mx4.git
cd mx4
cargo install --path .
```

### Run from source without installing

```bash
cargo run --release -- --help
cargo run --release -- status
cargo run --release -- firmware --json
```

The first `--` separates Cargo's arguments from arguments passed to mx4. To build once and run the resulting binary directly:

```bash
cargo build --release
./target/release/mx4 --help
```

## Library

This crate can also be used as a Rust library:

```rust
use mx4::features;

fn main() -> mx4::Result<()> {
    let battery = features::battery::read_status()?;
    println!("{} {}", battery.pct, battery.charging);
    Ok(())
}
```

## Usage

```bash
mx4 status
mx4 status --json
mx4 status battery
mx4 status battery --json
mx4 status dpi
mx4 status dpi --json
mx4 status wheel
mx4 status wheel --json
mx4 status thumb-wheel
mx4 status thumb-wheel --json
mx4 status force-button
mx4 status force-button --json
mx4 status haptic
mx4 status haptic --json
mx4 set host 2
mx4 set dpi 2500
mx4 set strength 100
mx4 set strength off
mx4 set wheel ratchet free
mx4 set wheel ratchet-speed 10
mx4 set wheel force 75
mx4 set wheel invert on
mx4 set wheel resolution off
mx4 set wheel divert off
mx4 set thumb-wheel invert off
mx4 set thumb-wheel divert on
mx4 set force-button 4310
mx4 daemon
mx4 daemon --install
mx4 daemon --once
mx4 daemon --uninstall
mx4 haptic 14
mx4 haptic 0..3
mx4 haptic '{0..14}'
mx4 battery
mx4 battery --json
mx4 firmware
mx4 firmware --json
```

Haptic strength presets:

- `off` = `0`
- `subtle` = `15`
- `low` = `45`
- `medium` = `60`
- `high` = `100`

## Background daemon

The daemon is opt-in. Normal status and setting commands never install or start a background service.

Install the binary first, then install and start the per-user service:

```bash
mx4 daemon --install
```

This creates a user `systemd` service on Linux or a user `launchd` agent on macOS. It watches for reconnects and reapplies saved settings. No `sudo` is needed.

Other daemon modes:

```bash
mx4 daemon             # run in the foreground
mx4 daemon --once      # apply saved settings once and exit
mx4 daemon --uninstall # stop and remove the background service
```

## Linux permissions

If `mx4 status` reports a `/dev/hidraw... Permission denied` error, install the udev rule once and reconnect the mouse or Logi Bolt receiver:

```bash
sudo install -Dm644 contrib/udev/99-mx4.rules /etc/udev/rules.d/99-mx4.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

The source checkout already contains that rule. A Cargo installation only installs the binary, so download the rule first if you do not have the source tree:

```bash
curl -fLO https://raw.githubusercontent.com/EriksRemess/mx4/main/contrib/udev/99-mx4.rules
sudo install -Dm644 99-mx4.rules /etc/udev/rules.d/99-mx4.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

`sudo mx4 status` can confirm that the HID++ device is readable, but it is only a diagnostic. Normal use should not need `sudo` after the udev rule is active.

Notes:

- `mx4 status` prints battery, DPI, wheel, thumb-wheel, force-button, and haptic state when available.
- `mx4 status --json` prints all readable status values as one JSON object, using `null` for unavailable features.
- `mx4 status haptic` prints the configured haptic level when the device exposes it.
- `mx4 battery` remains as a shorter alias for battery status.
- `mx4 firmware` prints the mouse firmware entities and Bolt receiver firmware when available.
- Persistent `mx4 set ...` commands save the applied value to a local config file so it can be restored later.
- `mx4 set host ...` and `mx4 haptic ...` do not persist anything.
- `mx4 daemon` polls for reconnects and reapplies saved settings when the mouse comes back.
- The background service is only installed when `mx4 daemon --install` is run explicitly.
- The saved config file lives at:
  - Linux: `~/.config/mx4/config.toml` unless `XDG_CONFIG_HOME` is set
  - macOS: `~/Library/Application Support/mx4/config.toml`
