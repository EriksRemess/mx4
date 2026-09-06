#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ServiceTest {
    root: PathBuf,
}

impl ServiceTest {
    fn new() -> Self {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "mx4-service-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        let script = root.join("bin/systemctl");
        fs::write(
            &script,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$MX4_TEST_LOG"
case "$2" in
    disable) exit "${MX4_TEST_DISABLE_EXIT:-0}" ;;
    show) printf '%s\n' "$MX4_TEST_STATE"; exit "${MX4_TEST_SHOW_EXIT:-0}" ;;
    restart) exit "${MX4_TEST_RESTART_EXIT:-0}" ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
        Self { root }
    }

    fn unit_path(&self) -> PathBuf {
        self.root.join("config/systemd/user/mx4.service")
    }

    fn seed_unit(&self) {
        let path = self.unit_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "[Service]\nExecStart=/old/mx4 daemon\n").unwrap();
    }

    fn run(&self, option: &str, variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx4"));
        command
            .args(["daemon", option])
            .env("PATH", self.root.join("bin"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("MX4_TEST_LOG", self.root.join("calls"))
            .env_remove("SUDO_USER")
            .env_remove("SUDO_UID");
        for (key, value) in variables {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.root.join("calls")).unwrap()
    }
}

impl Drop for ServiceTest {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn install_restarts_after_updating_unit_and_when_binary_path_is_unchanged() {
    let test = ServiceTest::new();
    test.seed_unit();
    for _ in 0..2 {
        assert!(test.run("--install", &[]).status.success());
    }
    assert!(
        !fs::read_to_string(test.unit_path())
            .unwrap()
            .contains("/old/mx4")
    );
    assert_eq!(
        test.calls(),
        "--user daemon-reload\n--user enable mx4.service\n--user restart mx4.service\n".repeat(2)
    );
}

#[test]
fn install_reports_restart_failure() {
    let test = ServiceTest::new();
    assert!(
        !test
            .run("--install", &[("MX4_TEST_RESTART_EXIT", "1")])
            .status
            .success()
    );
}

#[test]
fn uninstall_preserves_unit_on_stop_or_bus_failure() {
    for state in [
        "LoadState=loaded\nActiveState=active",
        "LoadState=not-found\nActiveState=active",
        "",
    ] {
        let test = ServiceTest::new();
        test.seed_unit();
        let result = test.run(
            "--uninstall",
            &[
                ("MX4_TEST_DISABLE_EXIT", "1"),
                ("MX4_TEST_STATE", state),
                ("MX4_TEST_SHOW_EXIT", "1"),
            ],
        );
        assert!(!result.status.success());
        assert!(test.unit_path().exists());
        assert!(!test.calls().contains("daemon-reload"));
    }
}

#[test]
fn uninstall_succeeds_for_stopped_and_missing_services() {
    let test = ServiceTest::new();
    test.seed_unit();
    assert!(test.run("--uninstall", &[]).status.success());
    assert!(!test.unit_path().exists());
    let result = test.run(
        "--uninstall",
        &[
            ("MX4_TEST_DISABLE_EXIT", "1"),
            (
                "MX4_TEST_STATE",
                "LoadState=not-found\nActiveState=inactive",
            ),
        ],
    );
    assert!(result.status.success());
}
