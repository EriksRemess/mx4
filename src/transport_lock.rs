//! Serialize HID++ exchanges across mx4 processes, including CLI/daemon combinations.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::Result;

pub(crate) fn acquire() -> Result<File> {
    // The configuration directory supplies the per-user namespace without querying OS user IDs.
    // Keep this separate from the saved-config lock and never unlink it: every process must lock the same file.
    acquire_at(&crate::config::config_path()?.with_file_name("transport.lock"))
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
