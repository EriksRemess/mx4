use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::Result;
use crate::config;
use crate::device;

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_millis(750);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(15);
const MISSING_POLLS_BEFORE_DISCONNECT: u8 = 2;

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
    let mut missing_polls = 0u8;
    let mut last_apply = None;

    loop {
        let now_connected = device::open().is_ok();

        if now_connected {
            missing_polls = 0;

            let should_apply = !connected
                || last_apply
                    .is_none_or(|instant: Instant| instant.elapsed() >= RECONCILE_INTERVAL);

            if should_apply {
                thread::sleep(RECONNECT_DELAY);
                apply_once();
                last_apply = Some(Instant::now());
            }

            connected = true;
        } else if connected {
            missing_polls = missing_polls.saturating_add(1);

            if missing_polls >= MISSING_POLLS_BEFORE_DISCONNECT {
                connected = false;
                last_apply = None;
            }
        }
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
