//! Serialize HID++ exchanges across mx4 processes, including CLI/daemon combinations.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::Result;

pub(crate) fn acquire() -> Result<File> {
    // The configuration directory supplies the per-user namespace without querying OS user IDs.
    // Keep this separate from the saved-config lock and never unlink it: every process must lock the same file.
    acquire_at(&crate::config::config_path()?.with_file_name("transport.lock"))
}

pub(crate) fn acquire_wheel() -> Result<File> {
    // Lock order: configuration -> wheel update -> individual transport exchange.
    // This separate lock lets a wheel update call req() repeatedly without recursive locking.
    acquire_at(&crate::config::config_path()?.with_file_name("wheel.lock"))
}

fn acquire_at(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("the HID++ lock path is not a regular file").into());
    }
    file.lock()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        process::{Command, Stdio},
    };

    #[cfg(target_os = "linux")]
    #[test]
    fn wheel_update_child() {
        let Some(bit) = std::env::var_os("MX4_TEST_WHEEL_BIT") else {
            return;
        };
        let bit: u32 = bit.to_str().unwrap().parse().unwrap();
        let directory = std::path::PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap());
        let _wheel = super::acquire_wheel().unwrap();
        // Model separate HID reads and writes: their transport locks must be acquirable while
        // the wheel guard protects the complete operation against other processes.
        let flags = {
            let _transport = super::acquire().unwrap();
            fs::read_to_string(directory.join("flags"))
                .unwrap()
                .parse::<u32>()
                .unwrap()
        };
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _transport = super::acquire().unwrap();
        fs::write(directory.join("flags"), (flags | (1 << bit)).to_string()).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wheel_updates_preserve_other_processes_flags() {
        let directory = std::env::temp_dir().join(format!("mx4-wheel-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("flags"), "0").unwrap();
        let children: Vec<_> = (0..8)
            .map(|bit| {
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "transport_lock::tests::wheel_update_child"])
                    .env("MX4_TEST_WHEEL_BIT", bit.to_string())
                    .env("XDG_CONFIG_HOME", &directory)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(fs::read_to_string(directory.join("flags")).unwrap(), "255");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn lock_child() {
        let Some(directory) = std::env::var_os("MX4_TEST_TRANSPORT_LOCK") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        for _ in 0..10 {
            let _lock = super::acquire_at(&directory.join("mx4/transport.lock")).unwrap();
            let counter = directory.join("counter");
            let value: u32 = fs::read_to_string(&counter).unwrap().parse().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            fs::write(counter, (value + 1).to_string()).unwrap();
        }
    }

    #[test]
    fn serializes_exchanges_across_processes() {
        let directory =
            std::env::temp_dir().join(format!("mx4-transport-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("counter"), "0").unwrap();
        let mut children: Vec<_> = (0..8)
            .map(|_| {
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "transport_lock::tests::lock_child"])
                    .env("MX4_TEST_TRANSPORT_LOCK", &directory)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(fs::read_to_string(directory.join("counter")).unwrap(), "80");
        fs::remove_dir_all(directory).unwrap();
    }
}
