//! `agent-sim simctl`: the part of Xcode's `xcrun simctl` that CI scripts and Isomer's iOS launcher use, for the
//! phones of the native arm64 boot (scripts/boot-arm64.sh), so those scripts run unchanged in a Linux container
//! (`xcrun` in the image is a link to agent-sim). A device's UDID is its phone's (sim.env's SIMULATOR_UDID); `booted`
//! is the first phone that runs. `create` starts a phone and prints its UDID once its device exists, `boot` starts it
//! if it does not run, `bootstatus -b` waits until it can run tests.

use crate::arm64;
use crate::device::Device;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUNTIME_ID: &str = "com.apple.CoreSimulator.SimRuntime.iOS-27-2";
const RUNTIME_DIR: &str = "ios-27.2-24B8117-arm64";
/// The phone the runtime is set up as (x86Sim's device: 1206 x 2622 pixels, at 3x).
const DEVICE_TYPES: [&str; 2] = ["iPhone 17", "iPhone 17 Pro"];

fn usage() -> anyhow::Error {
    anyhow::anyhow!(
        "usage: agent-sim simctl list|create|boot|bootstatus|shutdown|delete|install|uninstall|launch|terminate|\
         get_app_container|spawn|io|keychain|ui|status_bar ..."
    )
}

/// Every phone with a device directory, running or not.
fn phones() -> Result<Vec<Device>> {
    let var = arm64::x86sim_root()?.join("var");
    let mut found: Vec<Device> = std::fs::read_dir(&var)
        .map(|rd| {
            rd.flatten()
                .map(|e| Device { dir: e.path() })
                .filter(|d| arm64::number(d).is_some() && d.dir.join("sim.env").is_file())
                .collect()
        })
        .unwrap_or_default();
    found.sort_by_key(|d| arm64::number(d));
    Ok(found)
}

/// The phone a UDID (or `booted`) names.
fn find(udid: &str) -> Result<Device> {
    if udid == "booted" {
        return arm64::running_phones()?
            .into_iter()
            .next()
            .context("no phone is booted");
    }
    phones()?
        .into_iter()
        .find(|d| arm64::udid(d).is_some_and(|u| u.eq_ignore_ascii_case(udid)))
        .with_context(|| format!("no phone {udid}"))
}

fn number(dev: &Device) -> Result<usize> {
    arm64::number(dev).with_context(|| format!("{} is not a phone's device", dev.dir.display()))
}

fn runtime_root() -> Result<PathBuf> {
    let r = arm64::x86sim_root()?
        .join("runtimes")
        .join(RUNTIME_DIR)
        .join("RuntimeRoot");
    Ok(std::fs::canonicalize(&r).unwrap_or(r))
}

/// Whether the runtime is there: overlaid already, or where boot-arm64.sh overlays it from (AGENT_SIM_RUNTIME_RO, or
/// `<x86Sim>/runtimes-ro/<runtime>`).
fn runtime_available() -> Result<bool> {
    let x = arm64::x86sim_root()?;
    let ro = std::env::var_os("AGENT_SIM_RUNTIME_RO")
        .map(PathBuf::from)
        .unwrap_or_else(|| x.join("runtimes-ro").join(RUNTIME_DIR));
    Ok(runtime_root()?.is_dir() || ro.join("RuntimeRoot").exists())
}

/// A path the host gave, as the phone's processes see it (Darling's view of the Linux root).
fn device_view(p: &str) -> String {
    let abs = Path::new(p);
    let abs = if abs.is_absolute() {
        abs.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(abs)
    };
    format!("/Volumes/SystemRoot{}", abs.display())
}

/// Runs x86Sim's simctl.py for this phone, its output passed through; its exit status.
fn simctl_py(dev: &Device, args: &[String]) -> Result<i32> {
    let x = arm64::x86sim_root()?;
    let st = Command::new("python3")
        .arg(x.join("cli/libexec/simctl.py"))
        .args(args)
        .env("X86SIM_ROOT", &x)
        .env("X86SIM_DEVICE", &dev.dir)
        .stdin(Stdio::null())
        .status()
        .context("running simctl.py")?;
    Ok(st.code().unwrap_or(1))
}

fn list(args: &[String]) -> Result<i32> {
    let json = args.iter().any(|a| a == "-j" || a == "--json");
    let what = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("devices");
    let running = arm64::running_phones()?;
    let state = |d: &Device| {
        if running.iter().any(|r| r.dir == d.dir) {
            "Booted"
        } else {
            "Shutdown"
        }
    };
    let name = |d: &Device| {
        std::fs::read_to_string(d.dir.join(".simctl-name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| DEVICE_TYPES[0].to_string())
    };
    match what {
        "runtimes" => {
            let types: Vec<serde_json::Value> = DEVICE_TYPES
                .iter()
                .map(|t| serde_json::json!({ "name": t }))
                .collect();
            let rt = serde_json::json!({
                "identifier": RUNTIME_ID, "platform": "iOS", "version": "27.2", "buildversion": "24B8117",
                "name": "iOS 27.2", "isAvailable": runtime_available()?, "supportedDeviceTypes": types,
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "runtimes": [rt] }))?);
            } else {
                println!("== Runtimes ==\niOS 27.2 (27.2 - 24B8117) - {RUNTIME_ID}");
            }
        }
        "devicetypes" => {
            for t in DEVICE_TYPES {
                println!("{t}");
            }
        }
        _ => {
            let devices: Vec<serde_json::Value> = phones()?
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "udid": arm64::udid(d).unwrap_or_default(), "name": name(d), "state": state(d),
                        "isAvailable": true, "dataPath": d.dir.join("data"),
                    })
                })
                .collect();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "devices": { RUNTIME_ID: devices } }))?
                );
            } else {
                println!("== Devices ==\n-- iOS 27.2 --");
                for d in &devices {
                    println!(
                        "    {} ({}) ({})",
                        d["name"].as_str().unwrap_or(""),
                        d["udid"].as_str().unwrap_or(""),
                        d["state"].as_str().unwrap_or("")
                    );
                }
            }
        }
    }
    Ok(0)
}

/// `create <name> <device type> [runtime]`: the next free phone, started; its UDID once its device exists.
fn create(args: &[String]) -> Result<i32> {
    let name = args.first().ok_or_else(usage)?;
    let taken: Vec<usize> = phones()?.iter().filter_map(arm64::number).collect();
    let running: Vec<usize> = arm64::running_phones()?
        .iter()
        .filter_map(arm64::number)
        .collect();
    let n = (1..)
        .find(|n| !taken.contains(n) && !running.contains(n))
        .unwrap_or(1);
    arm64::start(n)?;
    let dev = arm64::phone(n)?;
    let start = Instant::now();
    loop {
        if let Some(u) = arm64::udid(&dev) {
            let _ = std::fs::write(dev.dir.join(".simctl-name"), name);
            println!("{u}");
            return Ok(0);
        }
        if start.elapsed() > Duration::from_secs(120) {
            bail!("{} did not come up", dev.dir.display());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Removes what a phone left: its device and its Darling prefix.
fn remove(dev: &Device) -> Result<()> {
    let n = number(dev)?;
    arm64::stop(n)?;
    std::thread::sleep(Duration::from_millis(500));
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    let prefix = if n == 1 {
        home.join(".darling")
    } else {
        home.join(format!(".darling-{n}"))
    };
    for d in [dev.dir.clone(), prefix] {
        if d.exists() {
            std::fs::remove_dir_all(&d).with_context(|| format!("removing {}", d.display()))?;
        }
    }
    Ok(())
}

/// `launch [--terminate-running-process] [--stdout=P] [--stderr=P] [-w] <udid> <bundle id> [args]`, the app's
/// environment from SIMCTL_CHILD_*: `<bundle id>: <pid>`. SpringBoard's launch brings the home screen to the front.
fn launch(args: &[String]) -> Result<i32> {
    let mut opts = Vec::new();
    let mut rest = args.iter();
    let mut positional = Vec::new();
    for a in rest.by_ref() {
        if let Some(p) = a.strip_prefix("--stdout=") {
            opts.push(format!("--stdout={}", device_view(p)));
        } else if let Some(p) = a.strip_prefix("--stderr=") {
            opts.push(format!("--stderr={}", device_view(p)));
        } else if a == "--terminate-running-process" || a == "-w" || a == "--wait-for-debugger" {
            opts.push(a.clone());
        } else if a.starts_with("--console") {
            opts.push(a.clone());
        } else {
            positional.push(a.clone());
            if positional.len() == 2 {
                break;
            }
        }
    }
    let [udid, bundle] = positional.as_slice() else {
        return Err(usage());
    };
    let dev = find(udid)?;
    if bundle == "com.apple.springboard" {
        crate::cmd::press(&dev, "home")?;
        println!("{bundle}: 1");
        return Ok(0);
    }
    let mut py = vec!["launch".to_string()];
    py.extend(opts);
    py.push(bundle.clone());
    py.extend(rest.cloned());
    simctl_py(&dev, &py)
}

/// `spawn <udid> <program> [args]`: a program of the runtime (found in its bin directories when named alone) run in the
/// phone.
fn spawn(args: &[String]) -> Result<i32> {
    let (Some(udid), Some(program)) = (args.first(), args.get(1)) else {
        return Err(usage());
    };
    let dev = find(udid)?;
    let path = if program.contains('/') {
        runtime_root()?.join(program.trim_start_matches('/'))
    } else {
        let rt = runtime_root()?;
        ["usr/bin", "bin", "usr/sbin", "sbin", "usr/libexec"]
            .iter()
            .map(|d| rt.join(d).join(program))
            .find(|p| p.is_file())
            .with_context(|| format!("no {program} in the runtime"))?
    };
    let mut py = vec!["spawn".to_string(), path.to_string_lossy().to_string()];
    py.extend(args[2..].iter().cloned());
    simctl_py(&dev, &py)
}

/// `io <udid> screenshot [--type=png] [--mask=black] <file>`: the screen as a PNG (the frame ring; the Dynamic Island
/// is drawn black by the phone itself).
fn io(args: &[String]) -> Result<i32> {
    let udid = args.first().ok_or_else(usage)?;
    if args.get(1).map(String::as_str) != Some("screenshot") {
        bail!("simctl io: only screenshot");
    }
    let file = args[2..]
        .iter()
        .rfind(|a| !a.starts_with('-'))
        .ok_or_else(usage)?;
    let dev = find(udid)?;
    // --mask=black (and alpha, here black too) paints outside the display's rounded corners, as Apple's does
    let black = args
        .iter()
        .any(|a| a == "--mask=black" || a == "--mask=alpha");
    std::fs::write(file, crate::cmd::screenshot_png_masked(&dev, black)?)
        .with_context(|| format!("writing {file}"))?;
    println!("Wrote screenshot to: {file}");
    Ok(0)
}

/// `keychain <udid> reset`: the phone's keychain emptied.
fn keychain(args: &[String]) -> Result<i32> {
    let (Some(udid), Some("reset")) = (args.first(), args.get(1).map(String::as_str)) else {
        bail!("simctl keychain: only reset");
    };
    let dev = find(udid)?;
    let dir = dev.dir.join("data/Library/Keychains");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(0)
}

/// `ui <udid> appearance [light|dark]`.
fn ui(args: &[String]) -> Result<i32> {
    let (Some(udid), Some("appearance")) = (args.first(), args.get(1).map(String::as_str)) else {
        bail!("simctl ui: only appearance");
    };
    let dev = find(udid)?;
    let Some(mode) = args.get(2) else {
        println!("light");
        return Ok(0);
    };
    let v = match mode.as_str() {
        "light" => "1",
        "dark" => "2",
        _ => bail!("simctl ui appearance: light or dark"),
    };
    let rt = runtime_root()?;
    let defaults = rt.join("usr/bin/defaults").to_string_lossy().to_string();
    let notify = rt.join("usr/bin/notifyutil").to_string_lossy().to_string();
    arm64::libexec(
        &dev,
        "simctl.py",
        &[
            "spawn",
            &defaults,
            "write",
            "com.apple.uikitservices.userInterfaceStyleMode",
            "UserInterfaceStyleMode",
            "-int",
            v,
        ],
    )?;
    arm64::libexec(
        &dev,
        "simctl.py",
        &[
            "spawn",
            &notify,
            "-p",
            "com.apple.uikitservices.userInterfaceStyleMode.changed",
        ],
    )?;
    Ok(0)
}

/// The time simctl's `--time` shows: an ISO 8601 date as its hour and minute ("2007-01-09T09:41:00.000-05:00" is
/// 9:41), anything else as it is.
fn shown_time(t: &str) -> String {
    let Some((_, clock)) = t.split_once('T') else {
        return t.to_string();
    };
    let mut parts = clock.split(':');
    match (
        parts.next().and_then(|h| h.parse::<u32>().ok()),
        parts.next().and_then(|m| m.get(..2)),
    ) {
        (Some(h), Some(m)) => format!("{h}:{m}"),
        _ => t.to_string(),
    }
}

/// `status_bar <udid> override [--time T] [--batteryLevel N] [--batteryState charging|charged|discharging] ...` or
/// `status_bar <udid> clear`, through agent-sim's hook in SpringBoard (the time and the battery; the network items are
/// the simulator's own, Wi-Fi at full).
fn status_bar(args: &[String]) -> Result<i32> {
    let (Some(udid), Some(what)) = (args.first(), args.get(1)) else {
        return Err(usage());
    };
    let dev = find(udid)?;
    if what == "clear" {
        println!("{}", crate::cmd::status_bar(&dev, None, None, false, true)?);
        return Ok(0);
    }
    if what != "override" {
        bail!("simctl status_bar: override, clear");
    }
    let (mut time, mut battery, mut charging) = (None, None, false);
    let mut it = args[2..].iter();
    while let Some(flag) = it.next() {
        let value = it.next().map(String::as_str).unwrap_or("");
        match flag.as_str() {
            "--time" => time = Some(shown_time(value)),
            "--batteryLevel" => battery = value.parse::<u8>().ok(),
            "--batteryState" => charging = value == "charging",
            _ => {}
        }
    }
    println!(
        "{}",
        crate::cmd::status_bar(&dev, time.as_deref(), battery, charging, false)?
    );
    Ok(0)
}

/// `agent-sim simctl ...`: Xcode's simctl, for the arm64 phones; the exit status simctl would give.
pub fn run(args: &[String]) -> Result<i32> {
    let (verb, rest) = args.split_first().ok_or_else(usage)?;
    let arg = |i: usize| rest.get(i).map(String::as_str).ok_or_else(usage);
    match verb.as_str() {
        "list" => list(rest),
        "create" => create(rest),
        "boot" => {
            let dev = find(arg(0)?)?;
            arm64::start(number(&dev)?)?;
            Ok(0)
        }
        "bootstatus" => {
            let dev = find(arg(0)?)?;
            let n = number(&dev)?;
            if rest.iter().any(|a| a == "-b") {
                arm64::start(n)?;
            }
            arm64::wait_ready(&[n], Duration::from_secs(900))?;
            Ok(0)
        }
        "shutdown" => {
            let targets = if arg(0)? == "all" {
                arm64::running_phones()?
            } else {
                vec![find(arg(0)?)?]
            };
            for d in targets {
                arm64::stop(number(&d)?)?;
            }
            Ok(0)
        }
        "delete" => {
            let targets = if arg(0)? == "all" {
                phones()?
            } else {
                vec![find(arg(0)?)?]
            };
            for d in targets {
                remove(&d)?;
            }
            Ok(0)
        }
        "install" => {
            println!("{}", arm64::install(&find(arg(0)?)?, Path::new(arg(1)?))?);
            Ok(0)
        }
        "uninstall" => {
            println!("{}", arm64::uninstall(&find(arg(0)?)?, arg(1)?)?);
            Ok(0)
        }
        "launch" => launch(rest),
        "terminate" => {
            let dev = find(arg(0)?)?;
            simctl_py(&dev, &["terminate".into(), arg(1)?.into()])
        }
        "get_app_container" => {
            let dev = find(arg(0)?)?;
            let mut py = vec!["get_app_container".to_string(), arg(1)?.to_string()];
            py.extend(rest.get(2).cloned());
            simctl_py(&dev, &py)
        }
        "spawn" => spawn(rest),
        "io" => io(rest),
        "keychain" => keychain(rest),
        "ui" => ui(rest),
        "status_bar" => status_bar(rest),
        _ => Err(usage()),
    }
}
