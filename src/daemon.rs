use std::thread;
use std::time::Duration;

use crate::Result;
use crate::config;
use crate::device;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const RECONNECT_DELAY: Duration = Duration::from_millis(750);

pub fn run(args: &[String]) -> Result<()> {
    match args {
        [] => run_loop(),
        [flag] if flag == "--once" => {
            apply_once();
            Ok(())
        }
        _ => Err("try `mx4 daemon` or `mx4 daemon --once`".into()),
    }
}

fn run_loop() -> Result<()> {
    let mut connected = false;

    loop {
        let now_connected = device::open().is_ok();

        if now_connected && !connected {
            thread::sleep(RECONNECT_DELAY);
            apply_once();
        }

        connected = now_connected;
        thread::sleep(POLL_INTERVAL);
    }
}

fn apply_once() {
    let config = match config::load() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("mx4 daemon: couldn't load config: {err}");
            return;
        }
    };

    if config.is_empty() {
        return;
    }

    for error in config::apply_best_effort(&config) {
        eprintln!("mx4 daemon: {error}");
    }
}
