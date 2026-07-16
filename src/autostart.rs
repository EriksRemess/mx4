//! Per-user background-service installation.
//!
//! Normal CLI use installs a small reconnect daemon on Linux or macOS. Generated service files are
//! only rewritten when their contents change, so ordinary commands do not reload the service.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

const SERVICE_NAME: &str = "mx4.service";
const LAUNCH_AGENT_LABEL: &str = "io.github.eriksremess.mx4";

pub fn ensure_installed() -> Result<()> {
    // Never create a root-owned per-user service when mx4 is being used through sudo. The explicit
    // environment switch is also used by the generated service to avoid recursively installing it.
    if should_skip_autostart(
        env::var_os("MX4_SKIP_AUTOSTART"),
        env::var_os("SUDO_USER"),
        env::var_os("SUDO_UID"),
    ) {
        return Ok(());
    }

    match env::consts::OS {
        "linux" => ensure_linux_service(),
        "macos" => ensure_macos_launch_agent(),
        _ => Ok(()),
    }
}

fn should_skip_autostart(
    explicit_skip: Option<std::ffi::OsString>,
    sudo_user: Option<std::ffi::OsString>,
    sudo_uid: Option<std::ffi::OsString>,
) -> bool {
    explicit_skip.is_some() || sudo_user.is_some() || sudo_uid.is_some()
}

fn ensure_linux_service() -> Result<()> {
    let service_dir = linux_service_dir()?;
    let service_path = service_dir.join(SERVICE_NAME);
    let executable = env::current_exe()?;
    let unit = linux_unit(&executable);
    let changed = write_if_changed(&service_path, &unit)?;

    if !changed {
        return Ok(());
    }

    run("systemctl", ["--user", "daemon-reload"])?;
    run("systemctl", ["--user", "enable", "--now", SERVICE_NAME])?;
    Ok(())
}

fn ensure_macos_launch_agent() -> Result<()> {
    let agent_dir = macos_launch_agents_dir()?;
    let agent_path = agent_dir.join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let executable = env::current_exe()?;
    let plist = macos_plist(&executable);
    let changed = write_if_changed(&agent_path, &plist)?;

    if !changed {
        return Ok(());
    }

    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let path = agent_path.to_string_lossy().into_owned();
    let service = format!("{domain}/{LAUNCH_AGENT_LABEL}");

    // A first install has nothing to boot out, and enable/kickstart are harmless conveniences after
    // a successful bootstrap, so only bootstrap is required to succeed.
    let _ = run_dynamic("launchctl", &["bootout", &domain, &path]);
    run_dynamic("launchctl", &["bootstrap", &domain, &path])?;
    let _ = run_dynamic("launchctl", &["enable", &service]);
    let _ = run_dynamic("launchctl", &["kickstart", "-k", &service]);
    Ok(())
}

fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Avoid touching timestamps and restarting an already-correct daemon on every CLI invocation.
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => Ok(false),
        Ok(_) | Err(_) => {
            fs::write(path, contents)?;
            Ok(true)
        }
    }
}

fn linux_service_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("couldn't determine HOME")?;
    Ok(env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"))
        .join("systemd")
        .join("user"))
}

fn macos_launch_agents_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("couldn't determine HOME")?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

fn linux_unit(executable: &Path) -> String {
    format!(
        "[Unit]\nDescription=mx4 reconnect daemon\nAfter=default.target\n\n[Service]\nType=simple\nEnvironment=MX4_SKIP_AUTOSTART=1\nExecStart={} daemon\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(executable),
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
  <key>EnvironmentVariables</key>
  <dict>
    <key>MX4_SKIP_AUTOSTART</key>
    <string>1</string>
  </dict>
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

fn run_dynamic(program: &str, args: &[&str]) -> Result<()> {
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

fn systemd_quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
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

    use super::{linux_unit, macos_plist, should_skip_autostart};

    #[test]
    fn systemd_unit_runs_daemon_mode() {
        let unit = linux_unit(Path::new("/tmp/mx4"));
        assert!(unit.contains("ExecStart=\"/tmp/mx4\" daemon"));
        assert!(unit.contains("MX4_SKIP_AUTOSTART=1"));
    }

    #[test]
    fn launch_agent_runs_daemon_mode() {
        let plist = macos_plist(Path::new("/tmp/mx4"));
        assert!(plist.contains("<string>/tmp/mx4</string>"));
        assert!(plist.contains("<string>daemon</string>"));
        assert!(plist.contains("MX4_SKIP_AUTOSTART"));
    }

    #[test]
    fn skips_autostart_under_sudo() {
        assert!(should_skip_autostart(None, Some("eriks".into()), None));
        assert!(should_skip_autostart(None, None, Some("1000".into())));
    }

    #[test]
    fn skips_autostart_when_explicitly_disabled() {
        assert!(should_skip_autostart(Some("1".into()), None, None));
    }
}
