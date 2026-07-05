use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");

    let version =
        resolved_dependency_version("Cargo.lock", "hidapi").unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=MX4_HIDAPI_VERSION={version}");
}

fn resolved_dependency_version(lockfile: impl AsRef<Path>, package: &str) -> Option<String> {
    let lockfile = fs::read_to_string(lockfile).ok()?;

    for section in lockfile.split("\n\n") {
        if !section.lines().any(|line| line == "[[package]]") {
            continue;
        }

        let mut name = None;
        let mut version = None;

        for line in section.lines() {
            if let Some(value) = lock_value(line, "name") {
                name = Some(value);
            } else if let Some(value) = lock_value(line, "version") {
                version = Some(value);
            }
        }

        if name == Some(package) {
            if let Some(version) = version {
                return Some(version.to_owned());
            }

            return None;
        }
    }

    None
}

fn lock_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (found_key, value) = line.split_once(" = ")?;
    if found_key != key {
        return None;
    }

    value.strip_prefix('"')?.strip_suffix('"')
}
