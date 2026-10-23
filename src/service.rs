//! Per-user background-service lifecycle management.
//!
//! The daemon is opt-in: these functions are called only by `mx4 daemon --install` and
//! `mx4 daemon --uninstall`. Generated files are per-user and never require root privileges.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::Result;

const SERVICE_NAME: &str = "mx4.service";
const LAUNCH_AGENT_LABEL: &str = "io.github.eriksremess.mx4";

/// Install and start the per-user daemon service using the current executable path.
///
/// Supports Linux systemd and macOS launchd. The executable must support `mx4 daemon`; library
/// callers should not use this to register an unrelated executable. Service-manager failures may
/// leave generated files in place so installation can be retried.
pub fn install() -> Result<()> {
    ensure_user_context()?;
    match env::consts::OS {
        "linux" => install_linux_service(),
        "macos" => install_macos_launch_agent(),
        os => Err(format!("background service installation isn't supported on {os}").into()),
    }
}

/// Stop and remove the per-user daemon service, retaining saved device settings.
///
/// Supports Linux systemd and macOS launchd; returns an error on unsupported platforms.
pub fn uninstall() -> Result<()> {
    ensure_user_context()?;
    match env::consts::OS {
        "linux" => uninstall_linux_service(),
        "macos" => uninstall_macos_launch_agent(),
        os => Err(format!("background service removal isn't supported on {os}").into()),
    }
}

fn ensure_user_context() -> Result<()> {
    if env::var_os("SUDO_USER").is_some() || env::var_os("SUDO_UID").is_some() {
        return Err("run daemon service commands without sudo".into());
    }
    Ok(())
}

fn install_linux_service() -> Result<()> {
    let service_path =
        installed_linux_service_path()?.unwrap_or(linux_service_dir()?.join(SERVICE_NAME));
    let executable = env::current_exe()?;
    let unit = linux_unit(&executable, &linux_config_home()?);
    write_if_changed(&service_path, &unit)?;
    // An explicit install also activates upgrades at the same executable path and retries a
    // previous failed reload. Enabling an already-active service alone would retain its old process.
    run("systemctl", ["--user", "daemon-reload"])?;
    // An absolute filename lets the manager link the unit into its own search path even when
    // the installer shell and user manager have different XDG_CONFIG_HOME values.
    run(
        "systemctl",
        [
            "--user",
            "enable",
            service_path.to_str().ok_or("service path is not UTF-8")?,
        ],
    )?;
    run("systemctl", ["--user", "restart", SERVICE_NAME])?;
    Ok(())
}

fn uninstall_linux_service() -> Result<()> {
    let service_path =
        installed_linux_service_path()?.unwrap_or(linux_service_dir()?.join(SERVICE_NAME));

    // Ignore failure only when the manager confirms there is no service left to stop. Preserve the
    // unit file on bus, permission, or stop failures so the user can retry the uninstall.
    if let Err(err) = run("systemctl", ["--user", "disable", "--now", SERVICE_NAME]) {
        let state = Command::new("systemctl")
            .args([
                "--user",
                "show",
                "--property=LoadState",
                "--property=ActiveState",
                SERVICE_NAME,
            ])
            .output()?;
        let state_text = String::from_utf8_lossy(&state.stdout);
        if !state.status.success()
            || !state_text.lines().any(|line| line == "LoadState=not-found")
            || !state_text
                .lines()
                .any(|line| line == "ActiveState=inactive")
        {
            return Err(err);
        }
    }
    if remove_if_exists(&service_path)? {
        run("systemctl", ["--user", "daemon-reload"])?;
    }
    Ok(())
}

fn install_macos_launch_agent() -> Result<()> {
    let agent_dir = macos_launch_agents_dir()?;
    let agent_path = agent_dir.join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let executable = env::current_exe()?;
    let plist = macos_plist(&executable);
    write_if_changed(&agent_path, &plist)?;

    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    install_launch_agent_at(&agent_path, &domain, |args| {
        Ok(Command::new("launchctl").args(args).output()?)
    })
}

fn install_launch_agent_at(
    path: &Path,
    domain: &str,
    mut launchctl: impl FnMut(&[&str]) -> Result<Output>,
) -> Result<()> {
    let path = path.to_str().ok_or("launch agent path is not UTF-8")?;
    let service = format!("{domain}/{LAUNCH_AGENT_LABEL}");
    // Disabled jobs cannot be bootstrapped. Clear the persistent override first.
    launchctl_checked(&["enable", &service], &mut launchctl)?;
    stop_launch_agent(domain, &mut launchctl)?;
    launchctl_checked(&["bootstrap", domain, path], &mut launchctl)?;
    launchctl_checked(&["kickstart", "-k", &service], &mut launchctl)
}

fn launchctl_checked(
    args: &[&str],
    launchctl: &mut impl FnMut(&[&str]) -> Result<Output>,
) -> Result<()> {
    let output = launchctl(args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "launchctl {} failed: {}: {}",
            args[0],
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into())
    }
}

fn uninstall_macos_launch_agent() -> Result<()> {
    let agent_path = macos_launch_agents_dir()?.join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    uninstall_launch_agent_at(&agent_path, &domain, |args| {
        Ok(Command::new("launchctl").args(args).output()?)
    })
}

fn uninstall_launch_agent_at(
    path: &Path,
    domain: &str,
    mut launchctl: impl FnMut(&[&str]) -> Result<Output>,
) -> Result<()> {
    stop_launch_agent(domain, &mut launchctl)?;
    remove_if_exists(path)?;
    Ok(())
}

fn stop_launch_agent(
    domain: &str,
    launchctl: &mut impl FnMut(&[&str]) -> Result<Output>,
) -> Result<()> {
    // Use the service target so a missing or modified plist cannot prevent stopping the job.
    let service = format!("{domain}/{LAUNCH_AGENT_LABEL}");
    let stopped = launchctl(&["bootout", &service])?;
    if !stopped.status.success() {
        // ESRCH means no such service. Confirm that its domain is reachable before accepting
        // absence; all other failures preserve the plist and remain visible to the caller.
        if stopped.status.code() != Some(3) || !launchctl(&["print", domain])?.status.success() {
            return Err(format!(
                "couldn't stop {service}: {}: {}",
                stopped.status,
                String::from_utf8_lossy(&stopped.stderr).trim()
            )
            .into());
        }
    }
    Ok(())
}

fn installed_linux_service_path() -> Result<Option<PathBuf>> {
    // Reuse the manager's existing unit across changes to the installer's XDG_CONFIG_HOME.
    // Enabling a new path would collide with the old regular unit in the manager's search path.
    let output = Command::new("systemctl")
        .args([
            "--user",
            "show",
            "--property=FragmentPath",
            "--value",
            SERVICE_NAME,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "couldn't locate {SERVICE_NAME}: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let value = String::from_utf8(output.stdout)?;
    let value = value.strip_suffix('\n').unwrap_or(&value);
    if value.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.file_name() != Some(std::ffi::OsStr::new(SERVICE_NAME)) {
        return Err(format!("refusing unexpected service path: {}", path.display()).into());
    }
    Ok(Some(path))
}

fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Avoid rewriting identical generated files.
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => Ok(false),
        Ok(_) | Err(_) => {
            fs::write(path, contents)?;
            Ok(true)
        }
    }
}

fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn linux_config_home() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("couldn't determine HOME")?;
    let path = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"));
    Ok(env::current_dir()?.join(path))
}

fn linux_service_dir() -> Result<PathBuf> {
    Ok(linux_config_home()?.join("systemd").join("user"))
}

fn macos_launch_agents_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("couldn't determine HOME")?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

fn linux_unit(executable: &Path, config_home: &Path) -> String {
    format!(
        "[Unit]\nDescription=mx4 reconnect daemon\nAfter=default.target\n\n[Service]\nType=simple\nEnvironment={}\nExecStart={} daemon\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(&format!("XDG_CONFIG_HOME={}", config_home.display())),
        systemd_quote(&executable.to_string_lossy()),
    )
}

fn macos_plist(executable: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>daemon</string>
  </array>
  <key>KeepAlive</key>
  <true/>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        LAUNCH_AGENT_LABEL,
        xml_escape(executable),
    )
}

fn run<const N: usize>(program: &str, args: [&str; N]) -> Result<()> {
    let status = Command::new(program).args(args).status()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with status {status}").into())
    }
}

fn current_uid() -> Result<String> {
    let output = Command::new("id").arg("-u").output()?;

    if !output.status.success() {
        return Err(format!("id -u exited with status {}", output.status).into());
    }

    let uid = String::from_utf8(output.stdout)?;
    let uid = uid.trim();

    if uid.is_empty() {
        return Err("id -u returned an empty uid".into());
    }

    Ok(uid.to_string())
}

fn systemd_quote(value: &str) -> String {
    use std::fmt::Write;

    let mut quoted = String::from("\"");
    for ch in value.chars() {
        match ch {
            '%' => quoted.push_str("%%"),
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            ch if ch.is_control() => {
                write!(quoted, "\\u{:04x}", ch as u32).expect("writing to String")
            }
            ch => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn xml_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{linux_unit, macos_plist};

    #[test]
    fn systemd_arguments_escape_specifiers_and_control_characters() {
        let unit = linux_unit(
            Path::new("/tmp/%n/line\nbreak\t\"\\/mx4"),
            Path::new("/tmp/%h/config\rdir"),
        );
        assert!(unit.contains("ExecStart=\"/tmp/%%n/line\\nbreak\\t\\\"\\\\/mx4\" daemon\n"));
        assert!(unit.contains("Environment=\"XDG_CONFIG_HOME=/tmp/%%h/config\\rdir\"\n"));
        assert_eq!(super::systemd_quote("\u{1b}\u{7f}"), "\"\\u001b\\u007f\"");
    }

    #[cfg(unix)]
    #[test]
    fn macos_install_enables_before_bootstrap_and_propagates_failures() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{ExitStatus, Output};

        for fail_at in [
            None,
            Some("enable"),
            Some("bootout"),
            Some("bootstrap"),
            Some("kickstart"),
        ] {
            let mut calls = Vec::new();
            let result =
                super::install_launch_agent_at(Path::new("/tmp/mx4.plist"), "gui/501", |args| {
                    calls.push(args[0].to_string());
                    Ok(Output {
                        status: ExitStatus::from_raw(if fail_at == Some(args[0]) {
                            5 << 8
                        } else {
                            0
                        }),
                        stdout: Vec::new(),
                        stderr: b"launchctl diagnostic".to_vec(),
                    })
                });
            let expected = ["enable", "bootout", "bootstrap", "kickstart"];
            let length = fail_at.map_or(expected.len(), |failed| {
                expected.iter().position(|arg| *arg == failed).unwrap() + 1
            });
            assert_eq!(calls, expected[..length]);
            assert_eq!(result.is_ok(), fail_at.is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn macos_uninstall_preserves_plist_unless_stopped_or_confirmed_absent() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{ExitStatus, Output};
        use std::sync::atomic::{AtomicU64, Ordering};

        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        for (bootout, domain, removed) in [
            (0, 0, true),
            (3, 0, true),
            (3, 1, false),
            (5, 0, false),
            (1, 0, false),
        ] {
            let path = std::env::temp_dir().join(format!(
                "mx4-launch-agent-test-{}-{}.plist",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, "original plist").unwrap();
            let mut calls = Vec::new();
            let result = super::uninstall_launch_agent_at(&path, "gui/501", |args| {
                calls.push(args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());
                Ok(Output {
                    status: ExitStatus::from_raw(
                        if args[0] == "bootout" {
                            bootout
                        } else {
                            domain
                        } << 8,
                    ),
                    stdout: Vec::new(),
                    stderr: b"launchctl diagnostic".to_vec(),
                })
            });
            assert_eq!(
                result.is_ok(),
                removed,
                "bootout={bootout}, domain={domain}"
            );
            assert_eq!(calls[0], ["bootout", "gui/501/io.github.eriksremess.mx4"]);
            if bootout == 3 {
                assert_eq!(calls[1], ["print", "gui/501"]);
            } else {
                assert_eq!(calls.len(), 1);
            }
            if !removed {
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "original plist");
                std::fs::remove_file(&path).unwrap();
            } else {
                assert!(!path.exists());
            }
        }
    }

    #[test]
    fn systemd_unit_runs_daemon_mode() {
        let unit = linux_unit(Path::new("/tmp/mx4"), Path::new("/tmp/config"));
        assert!(unit.contains("ExecStart=\"/tmp/mx4\" daemon"));
        assert!(!unit.contains("MX4_SKIP_AUTOSTART"));
    }

    #[test]
    fn launch_agent_runs_daemon_mode() {
        let plist = macos_plist(Path::new("/tmp/mx4"));
        assert!(plist.contains("<string>/tmp/mx4</string>"));
        assert!(plist.contains("<string>daemon</string>"));
        assert!(!plist.contains("MX4_SKIP_AUTOSTART"));
    }
}
