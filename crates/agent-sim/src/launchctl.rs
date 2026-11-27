//! `launchctl procinfo <pid>` for the arm64 phones (a link named launchctl, like xcrun's): whether iOS has suspended a
//! phone's process, which Isomer's iOS launcher asks a Mac's launchctl ("jetsam state = suspended") to know when to
//! bring an app back to the front. RunningBoard suspends a task through Mach's task_suspend, which Darling carries out
//! by signalling each of its threads and holding it in the signal's handler: a task whose every thread blocks that
//! signal (as a handler running does) is suspended.

use anyhow::{Context, Result, bail};
use std::path::Path;

/// The signal Darling holds a suspended thread in the handler of, as Linux numbers it (bit 33 of a thread's `SigBlk`
/// in /proc while it is held).
const SUSPEND_SIGNAL: u32 = 34;

pub fn run(args: &[String]) -> Result<i32> {
    let (Some("procinfo"), Some(pid)) = (args.first().map(String::as_str), args.get(1)) else {
        bail!("only `launchctl procinfo <pid>` here (agent-sim's iOS simulator on Linux)");
    };
    let pid: u32 = pid.parse().with_context(|| format!("not a pid: {pid}"))?;
    let Some(linux) = linux_pid(pid)? else {
        bail!("no phone process {pid}");
    };
    println!("pid = {pid}");
    println!(
        "jetsam state = {}",
        if suspended(linux)? {
            "suspended"
        } else {
            "running"
        }
    );
    Ok(0)
}

/// The Linux process of a phone's process `pid` (the pid iOS gives it, as `simctl launch` prints it): a Darling
/// process (mldr) whose innermost pid namespace knows it as `pid`. Each phone has a namespace of its own, so two phones
/// can each have a `pid`; an app's (in a bundle container) is taken over another's.
fn linux_pid(pid: u32) -> Result<Option<u32>> {
    let mut found = None;
    for e in std::fs::read_dir("/proc")?.flatten() {
        let Ok(linux) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if !std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == "mldr") {
            continue;
        }
        let Ok(status) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        if innermost_pid(&status) != Some(pid) {
            continue;
        }
        let cmd = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
        if String::from_utf8_lossy(&cmd).contains("/Containers/Bundle/Application/") {
            return Ok(Some(linux));
        }
        found.get_or_insert(linux);
    }
    Ok(found)
}

/// The last of a /proc status' `NSpid` pids: the process' pid in its own namespace.
fn innermost_pid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("NSpid:"))?
        .split_whitespace()
        .last()?
        .parse()
        .ok()
}

/// Whether every thread of Linux process `linux` blocks `SUSPEND_SIGNAL`: held suspended by Darling.
fn suspended(linux: u32) -> Result<bool> {
    let tasks = Path::new("/proc").join(linux.to_string()).join("task");
    let mut any = false;
    for e in std::fs::read_dir(&tasks)
        .with_context(|| format!("reading {}", tasks.display()))?
        .flatten()
    {
        let Ok(status) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        if !blocks_suspend_signal(&status) {
            return Ok(false);
        }
        any = true;
    }
    Ok(any)
}

/// Whether a /proc status' `SigBlk` mask holds `SUSPEND_SIGNAL` (signal n is bit n - 1).
fn blocks_suspend_signal(status: &str) -> bool {
    status
        .lines()
        .find_map(|l| l.strip_prefix("SigBlk:"))
        .and_then(|m| u64::from_str_radix(m.trim(), 16).ok())
        .is_some_and(|m| m & (1 << (SUSPEND_SIGNAL - 1)) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        // Visor sent home on a phone (every thread alike), in front, and a thread in another handler
        assert!(blocks_suspend_signal(
            "Name:\tmldr\nSigBlk:\t000000027ffbfeff\n"
        ));
        assert!(!blocks_suspend_signal("SigBlk:\t0000000000000000\n"));
        assert!(!blocks_suspend_signal("SigBlk:\t000000007ffbfeff\n"));
        assert!(!blocks_suspend_signal("Name:\tmldr\n"));
    }

    #[test]
    fn namespaced_pid() {
        assert_eq!(
            innermost_pid("Pid:\t1521357\nNSpid:\t1521357\t884\n"),
            Some(884)
        );
        assert_eq!(innermost_pid("NSpid:\t42\n"), Some(42));
        assert_eq!(innermost_pid("Pid:\t42\n"), None);
    }
}
