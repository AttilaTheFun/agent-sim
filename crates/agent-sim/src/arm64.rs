//! The arm64 phones (scripts/boot-arm64.sh): which run, booting more, and waiting until they can run tests.

use crate::device::Device;
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a phone's SpringBoard must have run before `wait_ready` starts asking it to launch an app: it is not
/// worth asking sooner, and a SpringBoard that keeps restarting never gets there.
const PROBE_AFTER_SECS: u64 = 60;

/// The app `wait_ready` launches to see whether the phone takes launches and touches yet.
const PROBE_APP: &str = "com.apple.Preferences";

/// The account's home directory, where the phones' Darling prefixes are: the password database's, not `$HOME`, which
/// `bazel test` points at a directory of its own.
pub fn home() -> Result<PathBuf> {
    // SAFETY: getpwuid's record is read at once, before any other call could replace it
    #[allow(unsafe_code)]
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_dir.is_null() {
            let dir = std::ffi::CStr::from_ptr((*pw).pw_dir)
                .to_string_lossy()
                .into_owned();
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }
    }
    Ok(PathBuf::from(
        std::env::var_os("HOME").context("HOME is not set")?,
    ))
}

/// x86Sim's checkout the arm64 phone runs from: `$X86SIM_ROOT`, else `~/x86sim` (scripts/setup-arm64.sh).
pub fn x86sim_root() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("X86SIM_ROOT") {
        return Ok(PathBuf::from(d));
    }
    Ok(home()?.join("x86sim"))
}

/// The pid file of the boot script starting a phone (scripts/boot-arm64.sh, which becomes the phone's `darling shell`
/// and runs until the phone stops).
fn boot_pid_file(x: &Path, name: &str) -> PathBuf {
    x.join(format!("var/boot-{name}.pid"))
}

/// The boot script's process for a phone, while it runs.
fn boot_pid(x: &Path, name: &str) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(boot_pid_file(x, name))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    (contains(&cmd, b"boot-arm64.sh") || contains(&cmd, b"darling")).then_some(pid)
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
/// The phone's SpringBoard: its Linux pid and how long it has run, in seconds.
fn springboard(udid: &str) -> Option<(u32, u64)> {
    let mark = format!("SIMULATOR_UDID={udid}");
    let uptime: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
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
        return Some((pid, (uptime - start / 100.0).max(0.0) as u64));
    }
    None
}

/// The file in which `wait_ready` notes the SpringBoard it found ready (its pid): a phone is ready while that
/// SpringBoard runs.
fn ready_file(dev: &Device) -> PathBuf {
    dev.dir.join("agent-sim-ready")
}

fn is_ready(dev: &Device, springboard_pid: u32) -> bool {
    std::fs::read_to_string(ready_file(dev)).is_ok_and(|s| s.trim() == springboard_pid.to_string())
}

/// Whether the phone takes an app launch: FrontBoard holds launches (and touches) until its system app has started,
/// two to three minutes after a boot under Darling (docs/ARM64.md), well after SpringBoard itself runs. Settings is
/// launched, closed again and the home screen brought back.
fn takes_launches(dev: &Device) -> bool {
    if launch(dev, PROBE_APP).is_err() {
        return false;
    }
    // closed once it runs: a terminate right after the launch came before the app was there to close, and Settings
    // stayed in the background (180 MB)
    let settings = || {
        crate::phone::process_ids(dev, |_, exe| exe == "Preferences").is_ok_and(|p| !p.is_empty())
    };
    let start = Instant::now();
    while !settings() && start.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(500));
    }
    while settings() && start.elapsed() < Duration::from_secs(45) {
        let _ = terminate(dev, PROBE_APP);
        std::thread::sleep(Duration::from_secs(1));
    }
    let _ = crate::cmd::press(dev, "home");
    // the home screen back before anything else is asked of the phone
    std::thread::sleep(Duration::from_secs(3));
    warm_touch(dev);
    true
}

/// Touches the phone once, on an empty part of the home screen, before anything else taps it: the first touch of a
/// boot was dropped (a tap on Settings' icon right after `phones up` did nothing for 20 s; the next one opened it in
/// half a second), as the keyboard's first key loses its case (`warm_keyboard`).
fn warm_touch(dev: &Device) {
    let _ = crate::cmd::tap(dev, &["201".to_string(), "650".to_string()], 80);
    std::thread::sleep(Duration::from_secs(1));
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
    let name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    // still booting (`simctl boot`, then `simctl bootstatus -b`): one boot at a time, or the second finds the first's
    // darlingserver and gives up
    if boot_pid(&x, &name).is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(x.join("var"))?;
    let log_path = x.join(format!("var/boot-{name}.log"));
    let log =
        File::create(&log_path).with_context(|| format!("creating {}", log_path.display()))?;
    let mut cmd = Command::new(&boot);
    cmd.env("X86SIM_ROOT", &x)
        .env("HOME", home()?)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if n > 1 {
        cmd.env("A64_PHONE", n.to_string());
    }
    // a process group of its own: the phone outlives this command
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd
        .spawn()
        .with_context(|| format!("starting {}", boot.display()))?;
    std::fs::write(boot_pid_file(&x, &name), child.id().to_string())?;
    eprintln!(
        "agent-sim phones: booting {name} (log: {})",
        log_path.display()
    );
    Ok(())
}

/// How many times `wait_ready` boots a phone again whose boot died.
const MAX_REBOOTS: u32 = 2;

/// Why phone `n`'s boot died, if it did: simhost saw launchd_sim killed (a SIGSEGV at the start of a boot, now and
/// then: Visor's proof, 2026-11-27), or the boot script that `start` ran is gone. A phone booted some other way (no
/// pid file) is never taken for dead.
fn boot_died(x: &Path, n: usize) -> Option<String> {
    let dir = device_dir(x, n);
    let name = dir.file_name()?.to_string_lossy().to_string();
    let log = std::fs::read_to_string(x.join(format!("var/boot-{name}.log"))).unwrap_or_default();
    if let Some(l) = log
        .lines()
        .find(|l| l.contains("launchd_sim killed by signal"))
    {
        return Some(l.trim().to_string());
    }
    if boot_pid_file(x, &name).is_file() && boot_pid(x, &name).is_none() {
        let last = log
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        return Some(format!("its boot script exited: {}", last.trim()));
    }
    None
}

/// Waits until phones `ns` can run tests (launchd_sim up, and SpringBoard launching apps: `takes_launches`), attaching each one's
/// keyboard once it is. A phone whose boot dies is stopped and booted again, up to `MAX_REBOOTS` times, rather than
/// waited on until `timeout`.
pub fn wait_ready(ns: &[usize], timeout: Duration) -> Result<()> {
    let x = x86sim_root()?;
    let start = Instant::now();
    let mut ready = vec![false; ns.len()];
    let mut reboots = vec![0; ns.len()];
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
            if !up {
                if let Some(why) = boot_died(&x, n) {
                    let name = dev
                        .dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if reboots[i] >= MAX_REBOOTS {
                        bail!(
                            "{name}'s boot died {} times, last: {why} (see <x86Sim>/var/boot-{name}.log)",
                            reboots[i] + 1
                        );
                    }
                    reboots[i] += 1;
                    eprintln!(
                        "agent-sim phones: {name}'s boot died ({why}); booting it again ({}/{MAX_REBOOTS})",
                        reboots[i]
                    );
                    stop(n)?;
                    self::start(n)?;
                    continue;
                }
            }
            let Some((pid, secs)) = udid(&dev).and_then(|u| springboard(&u)) else {
                continue;
            };
            if up && (is_ready(&dev, pid) || (secs >= PROBE_AFTER_SECS && takes_launches(&dev))) {
                ready[i] = true;
                let _ = std::fs::write(ready_file(&dev), pid.to_string());
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
    let home = home()?;
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
    let dir = device_dir(&x, n);
    let name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    if let Some(pid) = boot_pid(&x, &name) {
        // the script's process group: the script and what it started outside Darling
        // SAFETY: kill(2) with a process group we started; no memory is shared.
        #[allow(unsafe_code)]
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL)
        };
    }
    let _ = std::fs::remove_file(boot_pid_file(&x, &name));
    // killed, then gone: a boot right after must not find this one's processes (darlingserver takes its
    // processes down with it, not at once)
    let start = Instant::now();
    loop {
        let mut left = 0;
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
                left += 1;
                let _ = crate::phone::signal(pid, libc::SIGKILL);
            }
        }
        if left == 0 && !running_phones()?.iter().any(|p| p.dir == dir) {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(30) {
            bail!("{name}'s processes are still there 30 s after being killed");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
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
            let name = p
                .dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match udid(p).and_then(|u| springboard(&u)) {
                Some((pid, s)) if is_ready(p, pid) => {
                    format!("{name}: ready (SpringBoard up {s} s)")
                }
                Some((_, s)) => format!("{name}: starting (SpringBoard up {s} s)"),
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
        bail!(
            "{program} {}: {}",
            args.join(" "),
            if err.is_empty() { &text } else { err }
        );
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
