//! The commands an agent uses: boot, look, touch, type, apps.

use crate::ax;
use crate::device::Device;
use crate::input::Hid;
use crate::phone::{self, Screen};
use crate::ring::Ring;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn up(name: &str) -> Result<()> {
    phone::script("phone.sh", &["start", name], &[])?;
    phone::script("phone.sh", &["wait", name], &[])
}

pub fn down(name: &str) -> Result<()> {
    phone::script("phone.sh", &["stop", name], &[])
}

pub fn status() -> Result<()> {
    let var = crate::device::data_dir()?.join("var");
    let mut found = false;
    for e in std::fs::read_dir(&var)
        .with_context(|| format!("reading {}", var.display()))?
        .flatten()
    {
        let n = e.file_name().to_string_lossy().into_owned();
        let Some(name) = n.strip_prefix("device-a64-") else {
            continue;
        };
        let Ok(dev) = Device::find(name) else {
            continue;
        };
        found = true;
        let model = phone::sim_env(&dev)
            .ok()
            .and_then(|m| m.get("SIMULATOR_MODEL_IDENTIFIER").cloned())
            .unwrap_or_default();
        let state = if phone::is_running(&dev) {
            "running"
        } else {
            "stopped"
        };
        println!("{name}\t{state}\t{model}");
    }
    if !found {
        println!("no phones yet: run scripts/setup-vm.sh");
    }
    Ok(())
}

pub fn screenshot_png(dev: &Device) -> Result<Vec<u8>> {
    let mut ring = Ring::new(&dev.ring_path()?);
    let frame = ring
        .latest()
        .context("no frame yet: is the phone running?")?;
    let mut png_bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut png_bytes, frame.width as u32, frame.height as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut rgb = Vec::with_capacity(frame.width * frame.height * 3);
    for px in frame.bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(png_bytes)
}

pub fn screenshot(dev: &Device, out: Option<PathBuf>) -> Result<String> {
    let out = out.unwrap_or_else(|| PathBuf::from("screenshot.png"));
    std::fs::write(&out, screenshot_png(dev)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(out.display().to_string())
}

pub fn snapshot(dev: &Device, json: bool) -> Result<String> {
    let s = ax::snapshot(dev)?;
    Ok(if json {
        serde_json::to_string_pretty(&s)?
    } else {
        ax::format(&s).trim_end().to_string()
    })
}

/// Where a tap target is: `@e3`, `x y` in screen points, or text an element's label, value or id contains.
fn point(dev: &Device, target: &[String]) -> Result<(f64, f64, String)> {
    match target {
        [x, y] if x.parse::<f64>().is_ok() && y.parse::<f64>().is_ok() => {
            Ok((x.parse()?, y.parse()?, format!("({x},{y})")))
        }
        [r] if r.starts_with('@') => {
            let e = ax::resolve(dev, r)?;
            let (x, y) = e.center();
            Ok((x, y, describe(&e)))
        }
        [] => bail!("tap what? an element (@e3), a point (x y) or text"),
        words => {
            // the screen may still be changing (an app opening, a page sliding in): look again for a moment
            let text = words.join(" ");
            let start = Instant::now();
            loop {
                let s = ax::snapshot(dev)?;
                if let Some(e) = ax::find(&s, &text) {
                    let (x, y) = e.center();
                    return Ok((x, y, describe(e)));
                }
                if start.elapsed() > Duration::from_secs(3) {
                    bail!("nothing on screen matches \"{text}\"");
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }
}

fn describe(e: &ax::Element) -> String {
    match &e.label {
        Some(l) => format!("@{} {} \"{l}\"", e.reference, e.role),
        None => format!("@{} {}", e.reference, e.role),
    }
}

fn hid(dev: &Device) -> Hid {
    Hid::new(dev.hid_socket())
}

pub fn tap(dev: &Device, target: &[String], hold_ms: u64) -> Result<String> {
    let (x, y, what) = point(dev, target)?;
    let scr = Screen::of(dev)?;
    let (u, v) = scr.unit(x, y);
    let mut h = hid(dev);
    h.touch("down", u, v)?;
    std::thread::sleep(Duration::from_millis(hold_ms.max(80)));
    h.touch("up", u, v)?;
    Ok(format!("tapped {what}"))
}

pub fn swipe(dev: &Device, from: (f64, f64), to: (f64, f64), ms: u64) -> Result<String> {
    let scr = Screen::of(dev)?;
    let mut h = hid(dev);
    let steps = (ms / 16).clamp(4, 120);
    let (a, b) = scr.unit(from.0, from.1);
    h.touch("down", a, b)?;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let (x, y) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        let (u, v) = scr.unit(x, y);
        std::thread::sleep(Duration::from_millis(ms / steps));
        h.touch("move", u, v)?;
    }
    let (u, v) = scr.unit(to.0, to.1);
    h.touch("up", u, v)?;
    Ok(format!(
        "swiped ({:.0},{:.0}) to ({:.0},{:.0})",
        from.0, from.1, to.0, to.1
    ))
}

/// Scrolls the content: `down` shows what is below (the finger moves up), as on a phone.
pub fn scroll(dev: &Device, dir: &str, amount: f64) -> Result<String> {
    let scr = Screen::of(dev)?;
    let (cx, cy) = (scr.width / 2.0, scr.height / 2.0);
    let d = scr.height * 0.35 * amount.clamp(0.1, 2.0);
    let (from, to) = match dir {
        "down" => ((cx, cy + d / 2.0), (cx, cy - d / 2.0)),
        "up" => ((cx, cy - d / 2.0), (cx, cy + d / 2.0)),
        "left" => ((cx - d / 2.0, cy), (cx + d / 2.0, cy)),
        "right" => ((cx + d / 2.0, cy), (cx - d / 2.0, cy)),
        _ => bail!("scroll up, down, left or right"),
    };
    swipe(dev, from, to, 300)?;
    Ok(format!("scrolled {dir}"))
}

pub fn type_text(dev: &Device, text: &str) -> Result<String> {
    if text.chars().all(|c| crate::input::key_usage(c).is_some()) {
        hid(dev).text(text)?;
    } else {
        // characters without a key of their own go through x86Sim's typing
        phone::x86sim_ok(dev, &["type", text])?;
    }
    Ok(format!("typed {} characters", text.chars().count()))
}

pub fn press(dev: &Device, button: &str) -> Result<String> {
    match button {
        "home" | "lock" | "volup" | "voldown" => {
            let mut h = hid(dev);
            h.button(button, true)?;
            std::thread::sleep(Duration::from_millis(100));
            h.button(button, false)?;
        }
        _ => {
            phone::x86sim_ok(dev, &["button", button])?;
        }
    }
    Ok(format!("pressed {button}"))
}

pub fn launch(dev: &Device, bundle: &str, logs: bool) -> Result<String> {
    let args: Vec<&str> = if logs {
        vec!["launch", "--oslog", "--terminate-running-process", bundle]
    } else {
        vec!["launch", bundle]
    };
    // an app started right after it was stopped sometimes dies during startup: start it again once
    for attempt in 0..2 {
        let out = phone::x86sim_ok(dev, &args).map_err(|e| {
            if format!("{e:#}").contains("returned nil for") {
                anyhow::anyhow!("{bundle} is not installed (`agent-sim apps --all` lists what is)")
            } else {
                e
            }
        })?;
        let pid = out.split_whitespace().last().unwrap_or("?").to_string();
        if pid == "-1" {
            bail!("{bundle} did not start (is it installed? `agent-sim apps`)");
        }
        std::thread::sleep(Duration::from_millis(1500));
        let alive = phone::x86sim_run(dev, &["pidof", bundle])?.status.success();
        if alive || attempt == 1 {
            return Ok(format!("launched {bundle} (pid {pid})"));
        }
    }
    unreachable!()
}

pub fn terminate(dev: &Device, bundle: &str) -> Result<String> {
    if let Err(e) = phone::x86sim_ok(dev, &["terminate", bundle]) {
        if format!("{e:#}").contains("not running") {
            return Ok(format!("{bundle} was not running"));
        }
        return Err(e);
    }
    // x86Sim answers before the process has exited; a launch right away would race the old process
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let out = phone::x86sim_run(dev, &["pidof", bundle])?;
        if !out.status.success() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(format!("terminated {bundle}"))
}

pub fn open_url(dev: &Device, url: &str) -> Result<String> {
    phone::x86sim_ok(dev, &["openurl", url])?;
    Ok(format!("opened {url}"))
}

pub fn apps(dev: &Device, all: bool) -> Result<String> {
    let out = phone::x86sim_ok(dev, &["listapps", "--json"])?;
    let v: serde_json::Value = serde_json::from_str(&out).context("reading x86sim listapps")?;
    let map = v
        .as_object()
        .context("x86sim listapps: unexpected output")?;
    let mut rows: Vec<(String, String, String)> = map
        .iter()
        .filter_map(|(id, a)| {
            let kind = a.get("ApplicationType")?.as_str()?.to_string();
            let name = a
                .get("CFBundleDisplayName")
                .or_else(|| a.get("CFBundleName"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            (all || kind == "User").then(|| (id.clone(), name, kind))
        })
        .collect();
    rows.sort();
    if rows.is_empty() {
        return Ok(
            "no apps installed yet (`agent-sim install App.app|App.ipa`; --all lists the system's)"
                .into(),
        );
    }
    Ok(rows
        .into_iter()
        .map(|(id, name, kind)| format!("{id}\t{name}\t{}", kind.to_lowercase()))
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn install(dev: &Device, path: &Path) -> Result<String> {
    let app = crate::ipa::prepare(path)?;
    phone::x86sim_ok(dev, &["install", &app.path.to_string_lossy()])?;
    Ok(format!("installed {}", app.bundle_id))
}

pub fn uninstall(dev: &Device, bundle: &str) -> Result<String> {
    phone::x86sim_ok(dev, &["uninstall", bundle])?;
    Ok(format!("uninstalled {bundle}"))
}

/// Waits until an element whose label, value or id contains `text` is on screen (or, with `gone`, is not).
pub fn wait(dev: &Device, text: &str, timeout: Duration, gone: bool) -> Result<String> {
    let start = Instant::now();
    loop {
        let s = ax::snapshot(dev)?;
        let hit = ax::find(&s, text);
        match (hit, gone) {
            (Some(e), false) => return Ok(format!("found {}", describe(e))),
            (None, true) => return Ok(format!("\"{text}\" is gone")),
            _ => {}
        }
        if start.elapsed() > timeout {
            bail!(
                "{text:?} {} after {:.0} s",
                if gone {
                    "is still on screen"
                } else {
                    "did not appear"
                },
                timeout.as_secs_f64()
            );
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// The executable name of an installed app.
fn executable(dev: &Device, bundle: &str) -> Result<String> {
    let out = phone::x86sim_ok(dev, &["listapps", "--json"])?;
    let v: serde_json::Value = serde_json::from_str(&out).context("reading x86sim listapps")?;
    v.get(bundle)
        .and_then(|a| a.get("CFBundleExecutable"))
        .and_then(|e| e.as_str())
        .map(str::to_string)
        .with_context(|| format!("{bundle} is not installed"))
}

fn newest(dir: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max()
        .map(|(_, p)| p)
}

fn tail(path: &Path, lines: usize, matching: Option<&str>) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text
        .lines()
        .filter(|l| !l.contains("is implemented in both"))
        .filter(|l| matching.is_none_or(|m| l.contains(m)))
        .collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// An app's os_log messages (when it was launched with logs on) and the translator's report of its last run, which
/// names a crash's cause and guest backtrace.
pub fn logs(dev: &Device, bundle: &str, lines: usize, matching: Option<&str>) -> Result<String> {
    let exe = executable(dev, bundle)?;
    let mut out = String::new();
    let oslog = newest(
        &phone::data_dir(dev).join("var/log"),
        &format!("oslog.{exe}."),
    );
    match oslog {
        Some(p) => {
            out.push_str(&format!("== os_log ({})\n", p.display()));
            out.push_str(&tail(&p, lines, matching));
        }
        None => out.push_str(&format!(
            "== os_log: none yet (`agent-sim launch {bundle} --logs` starts the app with its os_log captured)"
        )),
    }
    let name = dev
        .dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let short = name.strip_prefix("device-a64-").unwrap_or(&name);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let run = home.join(format!(
        ".cache/x86sim/a64prefix-{short}/private/var/tmp/x86sim-a64"
    ));
    if let Some(p) = newest(&run, &format!("{exe}.")) {
        let report = tail(&p, lines.min(60), None);
        let crashed = report.contains("guest exception") || report.contains("killing pid");
        out.push_str(&format!(
            "\n\n== last run ({}){}\n{report}",
            p.display(),
            if crashed { ": crashed" } else { "" }
        ));
    }
    Ok(out)
}
