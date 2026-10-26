use std::process::Command;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ToolVersions {
    pub rustc: String,
    pub cmake: String,
}

pub fn tool_versions() -> ToolVersions {
    ToolVersions {
        rustc: command_output("rustc", &["--version"]),
        cmake: command_output("cmake", &["--version"])
            .lines()
            .next()
            .unwrap_or("unknown")
            .to_owned(),
    }
}

pub fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Serialize)]
pub struct Hardware {
    pub chip: String,
    pub architecture: String,
    pub macos: String,
    pub power: String,
}

pub fn hardware() -> Hardware {
    Hardware {
        chip: command_output("sysctl", &["-n", "machdep.cpu.brand_string"]),
        architecture: std::env::consts::ARCH.to_owned(),
        macos: command_output("sw_vers", &["-productVersion"]),
        power: power_state(),
    }
}

fn power_state() -> String {
    let output = command_output("pmset", &["-g", "batt"]);
    let Some(line) = output.lines().find(|line| line.contains('%')) else {
        return "unknown".into();
    };
    let details = line.split('\t').nth(1).unwrap_or(line);
    details
        .split(';')
        .take(2)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn command_output(program: &str, arguments: &[&str]) -> String {
    Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

pub fn current_rss_bytes() -> u64 {
    let pid = std::process::id().to_string();
    Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| output.trim().parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_mul(1024)
}

#[cfg(target_os = "macos")]
pub fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to valid writable storage for exactly one rusage
    // record. RUSAGE_SELF does not dereference any other application pointer.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return 0;
    }
    // SAFETY: a successful getrusage initialized the complete record. On macOS
    // ru_maxrss is reported in bytes (unlike Linux's KiB convention).
    let usage = unsafe { usage.assume_init() };
    usage.ru_maxrss.max(0) as u64
}

#[cfg(not(target_os = "macos"))]
pub fn peak_rss_bytes() -> u64 {
    0
}
