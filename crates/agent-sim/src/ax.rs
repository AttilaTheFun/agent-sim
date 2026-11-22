//! `agent-sim snapshot`: the screen as accessibility elements, with references (`@e3`) that `tap` accepts.
//!
//! hooks/ax.c in each phone process writes its visible elements as JSON when it gets SIGUSR2: SpringBoard (the home
//! screen, system alerts, the status bar, and front.json: the app in front) and that app.

use crate::device::Device;
use crate::phone;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    #[serde(rename = "ref", default)]
    pub reference: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    /// x, y, width, height in screen points
    pub frame: [f64; 4],
    /// the process the element belongs to (the app's executable, or SpringBoard)
    pub owner: String,
}

impl Element {
    pub fn center(&self) -> (f64, f64) {
        (
            self.frame[0] + self.frame[2] / 2.0,
            self.frame[1] + self.frame[3] / 2.0,
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// the app in front (bundle id), or None on the home screen
    pub app: Option<String>,
    /// the app in front did not answer (still starting, or busy): only SpringBoard's elements are listed
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub app_not_answering: bool,
    pub elements: Vec<Element>,
}

#[derive(Deserialize)]
struct RawDump {
    process: String,
    windows: Vec<RawWindow>,
}
#[derive(Deserialize)]
struct RawWindow {
    class: String,
    level: f64,
    elements: Vec<RawElement>,
}
#[derive(Deserialize)]
struct RawElement {
    label: Option<String>,
    value: Option<String>,
    hint: Option<String>,
    id: Option<String>,
    traits: Vec<String>,
    frame: [f64; 4],
}
#[derive(Deserialize)]
struct Front {
    bundle: Option<String>,
    pid: u32,
}

fn role(traits: &[String]) -> &'static str {
    let has = |t: &str| traits.iter().any(|x| x == t);
    if has("text-field") || has("search-field") {
        "textfield"
    } else if has("toggle") {
        "switch"
    } else if has("keyboard-key") {
        "key"
    } else if has("link") {
        "link"
    } else if has("button") {
        "button"
    } else if has("adjustable") {
        "adjustable"
    } else if has("tab-bar") {
        "tabbar"
    } else if has("header") {
        "heading"
    } else if has("image") {
        "image"
    } else if has("static-text") {
        "text"
    } else {
        "element"
    }
}

fn wait_for(path: &Path, timeout: Duration) -> Result<String> {
    let start = Instant::now();
    loop {
        if let Ok(s) = std::fs::read_to_string(path) {
            return Ok(s);
        }
        if start.elapsed() > timeout {
            bail!(
                "no answer from the phone ({} did not appear); are agent-sim's hooks loaded (scripts/phone.sh)?",
                path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Asks one process for its elements: `name` is the file it writes (ax-<pid>.json, SpringBoard ax-springboard.json).
fn dump(dir: &Path, linux_pid: u32, name: &str, timeout: Duration) -> Result<RawDump> {
    let file = dir.join(name);
    let _ = std::fs::remove_file(&file);
    phone::signal(linux_pid, libc::SIGUSR2)?;
    let text = wait_for(&file, timeout)?;
    serde_json::from_str(&text).with_context(|| format!("reading {}", file.display()))
}

pub fn snapshot(dev: &Device) -> Result<Snapshot> {
    let dir = phone::data_dir(dev).join("tmp/agent-sim");
    std::fs::create_dir_all(&dir)?;
    let sb = phone::processes(dev, |_, exe| exe == "SpringBoard")?;
    let sb = *sb.first().context("the phone is not running")?;
    let _ = std::fs::remove_file(dir.join("front.json"));
    let springboard = dump(&dir, sb, "ax-springboard.json", Duration::from_secs(5))?;
    wait_for(&dir.join("front.json"), Duration::from_secs(2))?;
    let front: Front = serde_json::from_str(&std::fs::read_to_string(dir.join("front.json"))?)?;
    let mut app_not_answering = false;
    let app = match (front.pid, &front.bundle) {
        (p, Some(_)) if p > 0 => {
            let pids = phone::processes(dev, |inner, _| inner == p)?;
            match pids.first() {
                Some(&linux) => {
                    match dump(&dir, linux, &format!("ax-{p}.json"), Duration::from_secs(3)) {
                        Ok(d) => Some(d),
                        Err(_) => {
                            app_not_answering = true;
                            None
                        }
                    }
                }
                None => None,
            }
        }
        _ => None,
    };

    // reading order: system alerts over everything, the app, the home screen when no app is in front, the status bar
    let mut parts: Vec<(&str, &RawWindow)> = Vec::new();
    let sb_windows = |pred: &dyn Fn(&RawWindow) -> bool| -> Vec<(&str, &RawWindow)> {
        let mut v: Vec<_> = springboard
            .windows
            .iter()
            .filter(|w| pred(w))
            .map(|w| ("SpringBoard", w))
            .collect();
        v.sort_by(|a, b| b.1.level.total_cmp(&a.1.level));
        v
    };
    parts.extend(sb_windows(&|w| {
        w.level > 1000.0 && !w.class.contains("StatusBar")
    }));
    if let Some(a) = &app {
        let mut v: Vec<_> = a.windows.iter().map(|w| (a.process.as_str(), w)).collect();
        v.sort_by(|x, y| y.1.level.total_cmp(&x.1.level));
        parts.extend(v);
    } else if !app_not_answering {
        parts.extend(sb_windows(&|w| {
            w.level <= 1000.0 && !w.class.contains("StatusBar")
        }));
    }
    parts.extend(sb_windows(&|w| w.class.contains("StatusBar")));

    let screen = phone::Screen::of(dev)?;
    let mut elements = Vec::new();
    for (owner, w) in parts {
        let home =
            owner == "SpringBoard" && (w.class.contains("HomeScreen") || w.class.contains("Dock"));
        for e in &w.elements {
            // off screen (a page scrolled away, a view sliding out): not something to read or tap
            let (cx, cy) = (e.frame[0] + e.frame[2] / 2.0, e.frame[1] + e.frame[3] / 2.0);
            if cx < 0.0 || cy < 0.0 || cx > screen.width || cy > screen.height {
                continue;
            }
            let mut states = Vec::new();
            if e.traits.iter().any(|t| t == "selected") {
                states.push("selected".to_string());
            }
            if e.traits.iter().any(|t| t == "not-enabled") {
                states.push("disabled".to_string());
            }
            elements.push(Element {
                reference: format!("e{}", elements.len() + 1),
                role: match role(&e.traits) {
                    "element" if home => "app",
                    r => r,
                }
                .to_string(),
                label: e.label.clone().filter(|s| !s.is_empty()),
                value: e.value.clone().filter(|s| !s.is_empty()),
                hint: e.hint.clone().filter(|s| !s.is_empty()),
                id: e.id.clone().filter(|s| !s.is_empty()),
                states,
                frame: e.frame,
                owner: owner.to_string(),
            });
        }
    }
    let snap = Snapshot {
        app: front.bundle.filter(|_| app.is_some() || app_not_answering),
        app_not_answering,
        elements,
    };
    std::fs::write(
        phone::state_dir(dev)?.join("snapshot.json"),
        serde_json::to_vec(&snap)?,
    )?;
    Ok(snap)
}

/// The elements of the last snapshot (what `@eN` refers to).
pub fn last(dev: &Device) -> Result<Snapshot> {
    let p = phone::state_dir(dev)?.join("snapshot.json");
    let text =
        std::fs::read_to_string(&p).context("no snapshot yet: run `agent-sim snapshot` first")?;
    Ok(serde_json::from_str(&text)?)
}

/// Text in double quotes, with only quotes, backslashes and line breaks escaped.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One line per element: `@e3 button "General"`, with its value, states and center point.
pub fn format(s: &Snapshot) -> String {
    let mut out = format!(
        "{}\n",
        match &s.app {
            Some(a) if s.app_not_answering =>
                format!("app {a} (not answering yet: starting or busy; snapshot again in a moment)"),
            Some(a) => format!("app {a}"),
            None => "home screen".to_string(),
        }
    );
    for e in &s.elements {
        let (x, y) = e.center();
        out.push_str(&format!("@{} {}", e.reference, e.role));
        if let Some(l) = &e.label {
            out.push_str(&format!(" {}", quote(l)));
        }
        if let Some(v) = &e.value {
            out.push_str(&format!(" value={}", quote(v)));
        }
        if let Some(i) = &e.id {
            out.push_str(&format!(" id={}", quote(i)));
        }
        for st in &e.states {
            out.push_str(&format!(" {st}"));
        }
        out.push_str(&format!(" ({x:.0},{y:.0})\n"));
    }
    out
}

/// Finds an element by reference (`@e3` / `e3`) in the last snapshot.
pub fn resolve(dev: &Device, r: &str) -> Result<Element> {
    let want = r.trim_start_matches('@');
    last(dev)?
        .elements
        .into_iter()
        .find(|e| e.reference == want)
        .with_context(|| format!("no element {r} in the last snapshot"))
}

/// The element `text` names: the first whose label, value or id equals it (ignoring case), else the first that
/// contains it.
pub fn find<'a>(s: &'a Snapshot, text: &str) -> Option<&'a Element> {
    let t = text.to_lowercase();
    let fields = |e: &'a Element| {
        [&e.label, &e.value, &e.id]
            .into_iter()
            .flatten()
            .map(|v| v.to_lowercase())
    };
    s.elements
        .iter()
        .find(|e| fields(e).any(|v| v == t))
        .or_else(|| {
            s.elements
                .iter()
                .find(|e| fields(e).any(|v| v.contains(&t)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_follow_traits() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(role(&t(&["button", "static-text"])), "button");
        assert_eq!(role(&t(&["search-field", "text-field"])), "textfield");
        assert_eq!(role(&t(&["static-text"])), "text");
        assert_eq!(role(&t(&[])), "element");
    }
}
