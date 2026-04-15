use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

const SERVICE_NAME: &str = "mx4.service";
const LAUNCH_AGENT_LABEL: &str = "io.github.eriksremess.mx4";

pub fn ensure_installed() -> Result<()> {
    if env::var_os("MX4_SKIP_AUTOSTART").is_some() {
        return Ok(());
    }

    match env::consts::OS {
        "linux" => ensure_linux_service(),
        "macos" => ensure_macos_launch_agent(),
        _ => Ok(()),
    }
}

fn ensure_linux_service() -> Result<()> {
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

fn ensure_macos_launch_agent() -> Result<()> {
    let agent_dir = macos_launch_agents_dir()?;
    let agent_path = agent_dir.join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let executable = env::current_exe()?;
    let plist = macos_plist(&executable);
    let _changed = write_if_changed(&agent_path, &plist)?;
    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let path = agent_path.to_string_lossy().into_owned();
    let service = format!("{domain}/{LAUNCH_AGENT_LABEL}");

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

    use super::{linux_unit, macos_plist};

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
}
