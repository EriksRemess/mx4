//! Reconnect watcher that restores persisted settings.
//!
//! The daemon polls for the active mouse transport, debounces brief disappearances, retries failed
//! writes, and periodically reconciles settings in case another application changed the device.

use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::{Result, config, device, service};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_millis(750);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(15);
const FAILURE_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const APPLY_ATTEMPTS: usize = 5;
const APPLY_RETRY_DELAY: Duration = Duration::from_secs(2);
const MISSING_POLLS_BEFORE_DISCONNECT: u8 = 2;

/// Run the CLI daemon mode selected by `args`.
///
/// No arguments starts a blocking reconnect loop. `--once` applies saved settings with retries;
/// `--install` and `--uninstall` manage the per-user service. Other arguments return an error.
pub fn run(args: &[String]) -> Result<()> {
    match args {
        [] => run_loop(),
        [flag] if flag == "--install" => service::install(),
        [flag] if flag == "--once" => apply_once_with_retry(),
        [flag] if flag == "--uninstall" => service::uninstall(),
        _ => Err(
            "try `mx4 daemon`, `mx4 daemon --install`, `mx4 daemon --once`, or `mx4 daemon --uninstall`"
                .into(),
        ),
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

            // The HID node can appear slightly before the mouse is ready for feature requests.
            if !connected {
                thread::sleep(RECONNECT_DELAY);
            }

            let should_apply =
                !connected || should_reconcile(last_apply_attempt, last_apply_success);

            if should_apply {
                let applied = match apply_once_with_retry() {
                    Ok(()) => true,
                    Err(err) => {
                        eprintln!("mx4 daemon: {err}");
                        false
                    }
                };
                let now = Instant::now();
                last_apply_attempt = Some(now);

                if applied {
                    last_apply_success = Some(now);
                }
            }

            connected = true;
        } else if connected {
            // Require two failed polls before declaring a disconnect; a single miss can occur while
            // the transport or kernel driver is briefly busy.
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
    // Failed writes retry quickly. Successful writes are refreshed less often to correct drift
    // without continuously sending traffic to the mouse.
    let retry_ready =
        last_apply_attempt.is_none_or(|instant| instant.elapsed() >= FAILURE_RETRY_INTERVAL);
    let reconcile_due =
        last_apply_success.is_none_or(|instant| instant.elapsed() >= RECONCILE_INTERVAL);

    retry_ready && reconcile_due
}

fn apply_once_with_retry() -> Result<()> {
    retry_settings(config::reconcile, || thread::sleep(APPLY_RETRY_DELAY))
}

fn retry_settings(
    mut apply: impl FnMut() -> Result<Vec<String>>,
    mut wait: impl FnMut(),
) -> Result<()> {
    let mut last_errors = Vec::new();

    // A reconnecting mouse may accept some features before others, so retry the complete best-effort
    // pass and report only the errors still present after the final attempt.
    for attempt in 0..APPLY_ATTEMPTS {
        // Each attempt reloads under the configuration lock; wait without holding any locks.
        let errors = apply()?;

        if errors.is_empty() {
            return Ok(());
        }

        last_errors = errors;

        if attempt + 1 != APPLY_ATTEMPTS {
            wait();
        }
    }

    Err(last_errors.join("; ").into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn retries_refresh_settings_after_waiting() {
        let desired = std::cell::Cell::new(2000);
        let mut applied = Vec::new();
        super::retry_settings(
            || {
                applied.push(desired.get());
                Ok(if applied.len() == 1 {
                    vec!["temporarily unavailable".into()]
                } else {
                    Vec::new()
                })
            },
            || desired.set(2500),
        )
        .unwrap();
        assert_eq!(applied, [2000, 2500]);
    }

    #[test]
    fn retries_are_bounded_and_report_the_last_failure() {
        let mut attempts = 0;
        let mut waits = 0;
        let error = super::retry_settings(
            || {
                attempts += 1;
                Ok(vec![format!("failure {attempts}")])
            },
            || waits += 1,
        )
        .unwrap_err();
        assert_eq!(attempts, super::APPLY_ATTEMPTS);
        assert_eq!(waits, attempts - 1);
        assert_eq!(error.to_string(), format!("failure {attempts}"));
    }
}
