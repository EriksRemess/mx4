//! Per-user background-service lifecycle management.
//!
//! The daemon is opt-in: these functions are called only by `mx4 daemon --install` and
//! `mx4 daemon --uninstall`. Generated files are per-user and never require root privileges.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

const SERVICE_NAME: &str = "mx4.service";
const LAUNCH_AGENT_LABEL: &str = "io.github.eriksremess.mx4";

pub fn install() -> Result<()> {
    ensure_user_context()?;
    match env::consts::OS {
        "linux" => install_linux_service(),
        "macos" => install_macos_launch_agent(),
        os => Err(format!("background service installation isn't supported on {os}").into()),
    }
}

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
    let service_dir = linux_service_dir()?;
    let service_path = service_dir.join(SERVICE_NAME);
    let executable = env::current_exe()?;
    let unit = linux_unit(&executable);
    let changed = write_if_changed(&service_path, &unit)?;

    if changed {
        run("systemctl", ["--user", "daemon-reload"])?;
    }

    run("systemctl", ["--user", "enable", "--now", SERVICE_NAME])?;
    Ok(())
}

fn uninstall_linux_service() -> Result<()> {
    let service_path = linux_service_dir()?.join(SERVICE_NAME);

    // A missing or already-stopped service is a successful uninstall.
    let _ = run("systemctl", ["--user", "disable", "--now", SERVICE_NAME]);
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

fn uninstall_macos_launch_agent() -> Result<()> {
    let agent_path = macos_launch_agents_dir()?.join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let path = agent_path.to_string_lossy().into_owned();

    let _ = run_dynamic("launchctl", &["bootout", &domain, &path]);
    remove_if_exists(&agent_path)?;
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

fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
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
        "[Unit]\nDescription=mx4 reconnect daemon\nAfter=default.target\n\n[Service]\nType=simple\nExecStart={} daemon\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
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

    use super::{linux_unit, macos_plist};

    #[test]
    fn systemd_unit_runs_daemon_mode() {
        let unit = linux_unit(Path::new("/tmp/mx4"));
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
