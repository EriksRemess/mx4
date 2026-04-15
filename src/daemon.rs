use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::Result;
use crate::config;
use crate::device;

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_millis(750);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(15);
const FAILURE_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const APPLY_ATTEMPTS: usize = 5;
const APPLY_RETRY_DELAY: Duration = Duration::from_secs(2);
const MISSING_POLLS_BEFORE_DISCONNECT: u8 = 2;

pub fn run(args: &[String]) -> Result<()> {
    match args {
        [] => run_loop(),
        [flag] if flag == "--once" => {
            apply_once_with_retry();
            Ok(())
        }
        _ => Err("try `mx4 daemon` or `mx4 daemon --once`".into()),
    }
}

fn run_loop() -> Result<()> {
    let mut connected = false;
    let mut missing_polls = 0u8;
    let mut last_apply_attempt: Option<Instant> = None;
    let mut last_apply_success: Option<Instant> = None;

    loop {
        let now_connected = device::open().is_ok();

        if now_connected {
            missing_polls = 0;

            if !connected {
                thread::sleep(RECONNECT_DELAY);
            }

            let should_apply =
                !connected || should_reconcile(last_apply_attempt, last_apply_success);

            if should_apply {
                let applied = apply_once_with_retry();
                let now = Instant::now();
                last_apply_attempt = Some(now);

                if applied {
                    last_apply_success = Some(now);
                }
            }

            connected = true;
        } else if connected {
            missing_polls = missing_polls.saturating_add(1);

            if missing_polls >= MISSING_POLLS_BEFORE_DISCONNECT {
                connected = false;
                last_apply_attempt = None;
                last_apply_success = None;
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn should_reconcile(
    last_apply_attempt: Option<Instant>,
    last_apply_success: Option<Instant>,
) -> bool {
    let retry_ready =
        last_apply_attempt.is_none_or(|instant| instant.elapsed() >= FAILURE_RETRY_INTERVAL);
    let reconcile_due =
        last_apply_success.is_none_or(|instant| instant.elapsed() >= RECONCILE_INTERVAL);

    retry_ready && reconcile_due
}

fn apply_once_with_retry() -> bool {
    let config = match config::load() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("mx4 daemon: couldn't load config: {err}");
            return false;
        }
    };

    if config.is_empty() {
        return true;
    }

    let mut last_errors = Vec::new();

    for attempt in 0..APPLY_ATTEMPTS {
        let errors = config::apply_best_effort(&config);

        if errors.is_empty() {
            return true;
        }

        last_errors = errors;

        if attempt + 1 != APPLY_ATTEMPTS {
            thread::sleep(APPLY_RETRY_DELAY);
        }
    }

    for error in last_errors {
        eprintln!("mx4 daemon: {error}");
    }

    false
}
