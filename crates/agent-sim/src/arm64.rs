//! The arm64 phones (scripts/boot-arm64.sh): which run, booting more, and waiting until they can run tests.

use crate::device::Device;
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a phone's SpringBoard must have run before the phone counts as ready: a first boot finishes starting
/// about two minutes after SpringBoard starts, and a SpringBoard that keeps restarting never gets there.
const READY_SECS: u64 = 120;

/// x86Sim's checkout the arm64 phone runs from: `$X86SIM_ROOT`, else `~/x86sim` (scripts/setup-arm64.sh).
pub fn x86sim_root() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("X86SIM_ROOT") {
        return Ok(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join("x86sim"))
}

/// The arm64 phones that run (scripts/boot-arm64.sh: `device-a64`, and `device-a64-<n>` for A64_PHONE=<n>): those
/// whose launchd_sim is up, found by its bootstrap plist on the command line.
pub fn running_phones() -> Result<Vec<Device>> {
    let var = x86sim_root()?.join("var");
    let mut cmdlines = Vec::new();
    for e in std::fs::read_dir("/proc")?.flatten() {
        if let Ok(c) = std::fs::read(e.path().join("cmdline")) {
            if contains(&c, b"launchd_sim") {
                cmdlines.push(String::from_utf8_lossy(&c).replace('\0', " "));
            }
        }
    }
    let mut phones = Vec::new();
    let Ok(rd) = std::fs::read_dir(&var) else {
        return Ok(phones);
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap_or_default().to_string_lossy();
            n == "device-a64" || n.starts_with("device-a64-")
        })
        .collect();
    dirs.sort();
    for d in dirs {
        let plist = format!("{}/data/var/run/launchd_bootstrap.plist", d.display());
        if d.join("sim.env").is_file() && cmdlines.iter().any(|c| c.contains(&plist)) {
            phones.push(Device { dir: d });
        }
    }
    Ok(phones)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The phone's simulator UDID (its sim.env), which its processes carry as SIMULATOR_UDID.
pub fn udid(dev: &Device) -> Option<String> {
    let env = std::fs::read_to_string(dev.dir.join("sim.env")).ok()?;
    env.lines()
        .find_map(|l| l.strip_prefix("SIMULATOR_UDID="))
        .map(str::to_string)
}

/// How long this phone's SpringBoard has been running, in seconds, or None if it is not.
fn springboard_secs(udid: &str) -> Option<u64> {
    let mark = format!("SIMULATOR_UDID={udid}");
    let uptime: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(cmd) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        if !contains(&cmd, b"SpringBoard.app/SpringBoard") {
            continue;
        }
        let Ok(env) = std::fs::read(e.path().join("environ")) else {
            continue;
        };
        if !env.split(|b| *b == 0).any(|v| v == mark.as_bytes()) {
            continue;
        }
        // /proc/<pid>/stat's 22nd field: the start time in clock ticks since boot (USER_HZ, 100 on Linux)
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        let after_name = &stat[stat.rfind(')')? + 2..];
        let start: f64 = after_name.split_whitespace().nth(19)?.parse().ok()?;
        return Some((uptime - start / 100.0).max(0.0) as u64);
    }
    None
}

/// The phone's device directory: `device-a64` for the first, `device-a64-<n>` for the others.
fn device_dir(x: &Path, n: usize) -> PathBuf {
    if n == 1 {
        x.join("var/device-a64")
    } else {
        x.join(format!("var/device-a64-{n}"))
    }
}

/// The phone's number: 1 for `device-a64`, n for `device-a64-<n>`.
pub fn number(dev: &Device) -> Option<usize> {
    let name = dev.dir.file_name()?.to_string_lossy().to_string();
    if name == "device-a64" {
        return Some(1);
    }
    name.strip_prefix("device-a64-")?.parse().ok()
}

/// The phone with this number, whether it exists yet or not.
pub fn phone(n: usize) -> Result<Device> {
    Ok(Device {
        dir: device_dir(&x86sim_root()?, n),
    })
}

/// Starts phone `n` (scripts/boot-arm64.sh in the background, its output in `<x86Sim>/var/boot-<device>.log`) unless
/// it runs already.
pub fn start(n: usize) -> Result<()> {
    let x = x86sim_root()?;
    let dir = device_dir(&x, n);
    if running_phones()?.iter().any(|p| p.dir == dir) {
        return Ok(());
    }
    let boot = crate::phone::scripts()?.join("boot-arm64.sh");
    if !boot.is_file() {
        bail!("no {}", boot.display());
    }
    let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
    std::fs::create_dir_all(x.join("var"))?;
    let log_path = x.join(format!("var/boot-{name}.log"));
    let log = File::create(&log_path).with_context(|| format!("creating {}", log_path.display()))?;
    let mut cmd = Command::new(&boot);
    cmd.env("X86SIM_ROOT", &x)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if n > 1 {
        cmd.env("A64_PHONE", n.to_string());
    }
    // a process group of its own: the phone outlives this command
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.spawn()
        .with_context(|| format!("starting {}", boot.display()))?;
    eprintln!("agent-sim phones: booting {name} (log: {})", log_path.display());
    Ok(())
}

/// Waits until phones `ns` can run tests (launchd_sim up, SpringBoard running for READY_SECS), attaching each one's
/// keyboard once it is.
pub fn wait_ready(ns: &[usize], timeout: Duration) -> Result<()> {
    let x = x86sim_root()?;
    let start = Instant::now();
    let mut ready = vec![false; ns.len()];
    loop {
        let running = running_phones()?;
        for (i, &n) in ns.iter().enumerate() {
            if ready[i] {
                continue;
            }
            let dev = Device {
                dir: device_dir(&x, n),
            };
            let up = running.iter().any(|p| p.dir == dev.dir);
            let secs = udid(&dev).and_then(|u| springboard_secs(&u)).unwrap_or(0);
            if up && secs >= READY_SECS {
                ready[i] = true;
                warm_keyboard(&dev);
                eprintln!(
                    "agent-sim phones: {} ready after {} s",
                    dev.dir.file_name().unwrap_or_default().to_string_lossy(),
                    start.elapsed().as_secs()
                );
            }
        }
        if ready.iter().all(|r| *r) {
            return Ok(());
        }
        if start.elapsed() > timeout {
            let late: Vec<String> = ns
                .iter()
                .zip(&ready)
                .filter(|(_, r)| !**r)
                .map(|(n, _)| device_dir(&x, *n).display().to_string())
                .collect();
            bail!(
                "not ready after {} s: {} (see <x86Sim>/var/boot-<device>.log)",
                timeout.as_secs(),
                late.join(", ")
            );
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// Stops phone `n`: its Darling server (every process of the phone goes with it), and the Metal and audio servers its
/// boot started.
pub fn stop(n: usize) -> Result<()> {
    let x = x86sim_root()?;
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    let prefix = if n == 1 {
        home.join(".darling")
    } else {
        home.join(format!(".darling-{n}"))
    };
    let state = if n == 1 {
        x.join("var/run")
    } else {
        x.join(format!("var/run-{n}"))
    };
    let server = format!("darlingserver {} ", prefix.display());
    let state = format!("{}/", state.display());
    for e in std::fs::read_dir("/proc")?.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(c) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let cmd = String::from_utf8_lossy(&c).replace('\0', " ");
        let ours = cmd.starts_with(&server)
            || ((cmd.contains("mtlvk") || cmd.contains("simaudio.py")) && cmd.contains(&state));
        if ours {
            let _ = crate::phone::signal(pid, libc::SIGKILL);
        }
    }
    Ok(())
}

/// Boots phones 1 to `count` that do not run (scripts/boot-arm64.sh, in the background), then waits until each can
/// run tests.
pub fn up(count: usize, timeout: Duration) -> Result<String> {
    if count == 0 {
        bail!("no phones asked for");
    }
    for n in 1..=count {
        start(n)?;
    }
    wait_ready(&(1..=count).collect::<Vec<_>>(), timeout)?;
    Ok(format!("{count} phone(s) ready"))
}

/// Attaches the phone's keyboard once, with a Shift press, before anything is typed: the first attach of a boot switches
/// UIKit into hardware-keyboard mode cold, and text typed right after it lost its case ("Ada" became "ADA").
fn warm_keyboard(dev: &Device) {
    let mut h = crate::input::Hid::new(dev.hid_socket());
    let _ = h.key(225, true);
    std::thread::sleep(Duration::from_millis(30));
    let _ = h.key(225, false);
}

/// The phones that run, each with how long its SpringBoard has been up.
pub fn status() -> Result<String> {
    let phones = running_phones()?;
    if phones.is_empty() {
        return Ok("no phone runs (agent-sim phones up)".to_string());
    }
    Ok(phones
        .iter()
        .map(|p| {
            let name = p.dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            match udid(p).and_then(|u| springboard_secs(&u)) {
                Some(s) if s >= READY_SECS => format!("{name}: ready (SpringBoard up {s} s)"),
                Some(s) => format!("{name}: starting (SpringBoard up {s} s)"),
                None => format!("{name}: starting (no SpringBoard yet)"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Whether this is a phone of the native arm64 boot (under `$X86SIM_ROOT/var`), rather than an x86Sim one.
pub fn is_native(dev: &Device) -> bool {
    x86sim_root()
        .map(|x| dev.dir.starts_with(x.join("var")))
        .unwrap_or(false)
}

/// Runs one of x86Sim's in-device helpers (`cli/libexec`) against this phone; its output, or its error.
pub fn libexec(dev: &Device, program: &str, args: &[&str]) -> Result<String> {
    let x = x86sim_root()?;
    let tool = x.join("cli/libexec").join(program);
    let mut cmd = if program.ends_with(".py") {
        let mut c = Command::new("python3");
        c.arg(&tool);
        c
    } else {
        Command::new(&tool)
    };
    let out = cmd
        .args(args)
        .env("X86SIM_ROOT", &x)
        .env("X86SIM_DEVICE", &dev.dir)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", tool.display()))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!("{program} {}: {}", args.join(" "), if err.is_empty() { &text } else { err });
    }
    Ok(text)
}

/// Installs a simulator .app, or the one in an .ipa or .zip.
pub fn install(dev: &Device, path: &Path) -> Result<String> {
    let work = dev
        .dir
        .join(format!("data/tmp/agent-sim-install/{}", std::process::id()));
    let app = crate::xctest::find_bundle(&crate::xctest::unpack(path, &work)?, "app")?;
    let r = libexec(dev, "appinstall.sh", &["install", &app.to_string_lossy()]);
    let _ = std::fs::remove_dir_all(&work);
    r
}

pub fn uninstall(dev: &Device, bundle_id: &str) -> Result<String> {
    libexec(dev, "appinstall.sh", &["uninstall", bundle_id])
}

pub fn launch(dev: &Device, bundle_id: &str) -> Result<String> {
    libexec(dev, "simctl.py", &["launch", bundle_id])
}

pub fn terminate(dev: &Device, bundle_id: &str) -> Result<String> {
    libexec(dev, "simctl.py", &["terminate", bundle_id])
}

/// The installed apps: `<bundle id> <type> <name>` lines, the user's only unless `all`.
pub fn apps(dev: &Device, all: bool) -> Result<String> {
    let list = libexec(dev, "simctl.py", &["listapps"])?;
    Ok(list
        .lines()
        .filter(|l| all || l.split_whitespace().nth(1) == Some("User"))
        .collect::<Vec<_>>()
        .join("\n"))
}
