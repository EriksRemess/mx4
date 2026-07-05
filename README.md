# mx4

Tiny CLI for MX Master 4 status and settings.

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
mx4 daemon --once
mx4 haptic 14
mx4 haptic 0..3
mx4 haptic '{0..14}'
mx4 battery
mx4 battery --json
```

Haptic strength presets:

- `off` = `0`
- `subtle` = `15`
- `low` = `45`
- `medium` = `60`
- `high` = `100`

## Linux permissions

If `mx4 status` reports a `/dev/hidraw... Permission denied` error, install the udev rule once and reconnect the mouse or Logi Bolt receiver:

```bash
sudo install -Dm644 contrib/udev/99-mx4.rules /etc/udev/rules.d/99-mx4.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

`sudo mx4 status` can confirm that the HID++ device is readable, but it is only a diagnostic. Normal use should not need `sudo` after the udev rule is active.

Notes:

- `mx4 status` prints battery, DPI, wheel, thumb-wheel, force-button, and haptic state when available.
- `mx4 status --json` prints all readable status values as one JSON object, using `null` for unavailable features.
- `mx4 status haptic` prints the configured haptic level when the device exposes it.
- `mx4 battery` remains as a shorter alias for battery status.
- Persistent `mx4 set ...` commands save the applied value to a local config file so it can be restored later.
- `mx4 set host ...` and `mx4 haptic ...` do not persist anything.
- `mx4 daemon` polls for reconnects and reapplies saved settings when the mouse comes back.
- On the first non-help run, `mx4` tries to install and start a background daemon automatically:
  - Linux: a user `systemd` service
  - macOS: a user `launchd` agent
- The saved config file lives at:
  - Linux: `~/.config/mx4/config.toml` unless `XDG_CONFIG_HOME` is set
  - macOS: `~/Library/Application Support/mx4/config.toml`
