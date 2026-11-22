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

pub fn screenshot(dev: &Device, out: Option<PathBuf>) -> Result<()> {
    let mut ring = Ring::new(&dev.ring_path()?);
    let frame = ring
        .latest()
        .context("no frame yet: is the phone running?")?;
    let out = out.unwrap_or_else(|| PathBuf::from("screenshot.png"));
    let file =
        std::fs::File::create(&out).with_context(|| format!("creating {}", out.display()))?;
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(file),
        frame.width as u32,
        frame.height as u32,
    );
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut rgb = Vec::with_capacity(frame.width * frame.height * 3);
    for px in frame.bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    enc.write_header()?.write_image_data(&rgb)?;
    println!("{}", out.display());
    Ok(())
}

pub fn snapshot(dev: &Device, json: bool) -> Result<()> {
    let s = ax::snapshot(dev)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&s)?);
    } else {
        print!("{}", ax::format(&s));
    }
    Ok(())
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
            let text = words.join(" ");
            let s = ax::snapshot(dev)?;
            let e = ax::find(&s, &text)
                .with_context(|| format!("nothing on screen matches {text:?}"))?;
            let (x, y) = e.center();
            Ok((x, y, describe(e)))
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

pub fn tap(dev: &Device, target: &[String], hold_ms: u64) -> Result<()> {
    let (x, y, what) = point(dev, target)?;
    let scr = Screen::of(dev)?;
    let (u, v) = scr.unit(x, y);
    let mut h = hid(dev);
    h.touch("down", u, v)?;
    std::thread::sleep(Duration::from_millis(hold_ms.max(80)));
    h.touch("up", u, v)?;
    println!("tapped {what}");
    Ok(())
}

pub fn swipe(dev: &Device, from: (f64, f64), to: (f64, f64), ms: u64) -> Result<()> {
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
    Ok(())
}

/// Scrolls the content: `down` shows what is below (the finger moves up), as on a phone.
pub fn scroll(dev: &Device, dir: &str, amount: f64) -> Result<()> {
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
    println!("scrolled {dir}");
    Ok(())
}

pub fn type_text(dev: &Device, text: &str) -> Result<()> {
    if text.chars().all(|c| crate::input::key_usage(c).is_some()) {
        hid(dev).text(text)?;
    } else {
        // characters without a key of their own go through x86Sim's typing
        phone::x86sim_ok(dev, &["type", text])?;
    }
    println!("typed {} characters", text.chars().count());
    Ok(())
}

pub fn press(dev: &Device, button: &str) -> Result<()> {
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
    println!("pressed {button}");
    Ok(())
}

pub fn launch(dev: &Device, bundle: &str) -> Result<()> {
    let out = phone::x86sim_ok(dev, &["launch", bundle])?;
    let pid = out.split_whitespace().last().unwrap_or("?");
    if pid == "-1" {
        bail!("{bundle} did not start (is it installed? `agent-sim apps`)");
    }
    println!("launched {bundle} (pid {pid})");
    Ok(())
}

pub fn terminate(dev: &Device, bundle: &str) -> Result<()> {
    phone::x86sim_ok(dev, &["terminate", bundle])?;
    println!("terminated {bundle}");
    Ok(())
}

pub fn open_url(dev: &Device, url: &str) -> Result<()> {
    phone::x86sim_ok(dev, &["openurl", url])?;
    println!("opened {url}");
    Ok(())
}

pub fn apps(dev: &Device, all: bool) -> Result<()> {
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
        println!(
            "no apps installed yet (`agent-sim install App.app|App.ipa`; --all lists the system's)"
        );
    }
    for (id, name, kind) in rows {
        println!("{id}\t{name}\t{}", kind.to_lowercase());
    }
    Ok(())
}

pub fn install(dev: &Device, path: &Path) -> Result<()> {
    let app = crate::ipa::prepare(path)?;
    let out = phone::x86sim_ok(dev, &["install", &app.path.to_string_lossy()])?;
    let _ = out;
    println!("installed {}", app.bundle_id);
    Ok(())
}

pub fn uninstall(dev: &Device, bundle: &str) -> Result<()> {
    phone::x86sim_ok(dev, &["uninstall", bundle])?;
    println!("uninstalled {bundle}");
    Ok(())
}

/// Waits until an element whose label, value or id contains `text` is on screen (or, with `gone`, is not).
pub fn wait(dev: &Device, text: &str, timeout: Duration, gone: bool) -> Result<()> {
    let start = Instant::now();
    loop {
        let s = ax::snapshot(dev)?;
        let hit = ax::find(&s, text);
        match (hit, gone) {
            (Some(e), false) => {
                println!("found {}", describe(e));
                return Ok(());
            }
            (None, true) => {
                println!("{text:?} is gone");
                return Ok(());
            }
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
