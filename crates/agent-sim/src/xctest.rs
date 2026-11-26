//! `agent-sim xctest`: runs an XCTest bundle on a phone, as `bazel test` asks a test runner to (rules_apple's
//! `AppleTestRunnerInfo`): a logic bundle on its own, a unit test bundle in its host app, or a UI test bundle driving
//! its app. The bundle and the app come as rules_apple builds them (a `.zip` and an `.ipa`) or unpacked. x86Sim's
//! runner (`cli/libexec/xctest.py`) does the running; this unpacks the inputs where the phone sees them, makes the UI
//! test runner app as Xcode does, holds the phone so two tests never share it, cleans up after and hands the results
//! to Bazel (`XML_OUTPUT_FILE`, `TEST_UNDECLARED_OUTPUTS_DIR`). docs/ARM64.md, docs/ARM64-ROADMAP.md (M3).

use crate::arm64::{running_phones, x86sim_root};
use crate::device::Device;
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// No host app: Xcode's `xctest` agent loads the bundle.
    Logic,
    /// A unit test bundle injected into its host app.
    Hosted,
    /// A UI test bundle in a runner app, driving the app under test.
    Ui,
}

pub struct Options {
    /// The test bundle: an `.xctest` or the `.zip` rules_apple makes of one.
    pub bundle: PathBuf,
    /// The host app (unit tests) or the app under test (UI tests): an `.app` or an `.ipa`.
    pub app: Option<PathBuf>,
    pub kind: Kind,
    /// `Target[/Class[/test]]` or `Class[/test]`, as xcodebuild's -only-testing:.
    pub only: Vec<String>,
    pub junit: Option<PathBuf>,
    /// Logs and results.json (default: a fresh directory under the device's tmp).
    pub out: Option<PathBuf>,
    /// Seconds per test target.
    pub timeout: u64,
    /// Outcomes the tests are meant to have (`check_expected`): the exit status then says whether they had them.
    pub expect: Option<PathBuf>,
}

/// rules_apple's `%(test_type)s` (`XCTEST`, `XCUITEST`) with or without a host app, as a kind.
pub fn kind_of(test_type: &str, has_app: bool) -> Result<Kind> {
    match test_type.to_ascii_uppercase().as_str() {
        "XCUITEST" | "UI" => Ok(Kind::Ui),
        "XCTEST" | "UNIT" | "" => Ok(if has_app { Kind::Hosted } else { Kind::Logic }),
        "LOGIC" => Ok(Kind::Logic),
        "HOSTED" => Ok(Kind::Hosted),
        t => bail!("unknown test type {t} (XCTEST or XCUITEST)"),
    }
}

/// Locks a phone for this test, without waiting: None if another test has it.
fn try_lock(dev: &Device) -> Result<Option<File>> {
    let path = dev.dir.join(".agent-sim-xctest.lock");
    let f = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    // SAFETY: flock(2) on a descriptor this File owns for as long as the lock is wanted; no memory is shared.
    #[allow(unsafe_code)]
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    Ok((r == 0).then_some(f))
}

/// A phone for this test, held (the returned lock) for the whole run, so two tests never share one: the one named,
/// else whichever running phone is free first. Tests wait for a phone rather than fail.
fn take_phone(named: Option<&Device>, skip: &[PathBuf]) -> Result<(Device, File)> {
    let mut said = false;
    loop {
        let phones: Vec<Device> = match named {
            Some(d) => vec![d.clone()],
            None => running_phones()?
                .into_iter()
                .filter(|p| !skip.contains(&p.dir))
                .collect(),
        };
        if phones.is_empty() {
            bail!("no phone runs: boot one with scripts/boot-arm64.sh (A64_PHONE=<n> for more)");
        }
        for p in phones {
            if let Some(lock) = try_lock(&p)? {
                return Ok((p, lock));
            }
        }
        if !said {
            eprintln!("agent-sim xctest: waiting for a phone (other tests have them)");
            said = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn run(cmd: &mut Command) -> Result<std::process::ExitStatus> {
    cmd.status()
        .with_context(|| format!("running {:?}", cmd.get_program()))
}

/// Unpacks `archive` (`.zip`, `.ipa`) into `into`, or returns a bundle given unpacked.
fn unpack(archive: &Path, into: &Path) -> Result<PathBuf> {
    if archive.is_dir() {
        return Ok(archive.to_path_buf());
    }
    std::fs::create_dir_all(into)?;
    let st = run(Command::new("unzip")
        .arg("-qo")
        .arg(archive)
        .arg("-d")
        .arg(into))?;
    if !st.success() {
        bail!("unzip {} failed ({st})", archive.display());
    }
    Ok(into.to_path_buf())
}

/// The one bundle with this extension in `dir` (or `dir/Payload`, as in an .ipa), or `dir` itself.
fn find_bundle(dir: &Path, ext: &str) -> Result<PathBuf> {
    if dir.extension() == Some(OsStr::new(ext)) {
        return Ok(dir.to_path_buf());
    }
    for d in [dir.join("Payload"), dir.to_path_buf()] {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut found: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension() == Some(OsStr::new(ext)))
            .collect();
        found.sort();
        if let Some(p) = found.into_iter().next() {
            return Ok(p);
        }
    }
    bail!("no .{ext} in {}", dir.display())
}

fn bundle_id(bundle: &Path) -> Result<String> {
    let info: plist::Dictionary = plist::from_file(bundle.join("Info.plist"))
        .with_context(|| format!("reading {}/Info.plist", bundle.display()))?;
    info.get("CFBundleIdentifier")
        .and_then(|v| v.as_string())
        .map(str::to_string)
        .with_context(|| format!("{} has no CFBundleIdentifier", bundle.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    let st = run(Command::new("cp").arg("-a").arg(from).arg(to))?;
    if !st.success() {
        bail!("copying {} to {} failed", from.display(), to.display());
    }
    Ok(())
}

/// A UI test bundle's runner app, made as Xcode makes `<Bundle>-Runner.app`: Xcode's XCTRunner.app with its executable
/// and its Info.plist's `$(WRAPPED...)` placeholders named for the bundle, and the bundle in its PlugIns.
fn ui_runner(platforms: &Path, bundle: &Path, work: &Path) -> Result<PathBuf> {
    let template =
        platforms.join("iPhoneSimulator.platform/Developer/Library/Xcode/Agents/XCTRunner.app");
    if !template.is_dir() {
        bail!(
            "no {}: XCTest's platform comes from an Xcode .xip (XCODE_XIP=... scripts/setup-arm64.sh)",
            template.display()
        );
    }
    let stem = bundle
        .file_stem()
        .context("test bundle without a name")?
        .to_string_lossy()
        .to_string();
    let name = format!("{stem}-Runner");
    let runner = work.join(format!("{name}.app"));
    copy_dir(&template, &runner)?;
    std::fs::rename(runner.join("XCTRunner"), runner.join(&name))
        .context("naming the runner's executable")?;
    let info_path = runner.join("Info.plist");
    let mut info: plist::Dictionary =
        plist::from_file(&info_path).with_context(|| format!("reading {}", info_path.display()))?;
    let runner_id = format!("{}.xctrunner", bundle_id(bundle)?);
    for (_, v) in info.iter_mut() {
        let replaced = match v.as_string() {
            Some("$(WRAPPEDPRODUCTNAME)") => Some(name.clone()),
            Some("$(WRAPPEDPRODUCTBUNDLEIDENTIFIER)") => Some(runner_id.clone()),
            _ => None,
        };
        if let Some(s) = replaced {
            *v = plist::Value::String(s);
        }
    }
    plist::to_file_binary(&info_path, &info)
        .with_context(|| format!("writing {}", info_path.display()))?;
    let plugins = runner.join("PlugIns");
    std::fs::create_dir_all(&plugins)?;
    copy_dir(bundle, &plugins)?;
    Ok(runner)
}

/// Removes an app from the phone (x86Sim's appinstall.sh), quietly: the next test starts from a clean phone.
fn uninstall(x: &Path, dev: &Device, bid: &str) {
    let _ = Command::new(x.join("cli/libexec/appinstall.sh"))
        .args(["uninstall", bid])
        .env("X86SIM_ROOT", x)
        .env("X86SIM_DEVICE", &dev.dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Runs the tests on the phone named, else on whichever is free; returns xctest.py's exit status: 0 when every test
/// passed or was skipped, 1 when one failed, 2 when nothing could run. A phone that stops during the run (its Darling
/// server gone, and launchd_sim with it) leaves the results meaningless: the test runs once more on another phone, if
/// one is booted.
pub fn xctest(named: Option<Device>, o: &Options) -> Result<i32> {
    let x = x86sim_root()?;
    let runner_py = x.join("cli/libexec/xctest.py");
    if !runner_py.is_file() {
        bail!(
            "no {} (scripts/setup-arm64.sh sets x86Sim up)",
            runner_py.display()
        );
    }
    let platforms = x.join("sdks");
    let mut stopped = Vec::new();
    loop {
        let (phone, _lock) = take_phone(named.as_ref(), &stopped)?;
        let code = run_on(&phone, o, &x, &runner_py, &platforms)?;
        let running = running_phones()?;
        let alive = running.iter().any(|p| p.dir == phone.dir);
        let another = running.iter().any(|p| p.dir != phone.dir);
        if code != 0 && !alive && another && named.is_none() && stopped.is_empty() {
            eprintln!(
                "agent-sim xctest: {} stopped during the test; running it again on another phone",
                phone.dir.display()
            );
            stopped.push(phone.dir.clone());
            continue;
        }
        return Ok(code);
    }
}

/// One run of the tests on a phone this test holds.
fn run_on(dev: &Device, o: &Options, x: &Path, runner_py: &Path, platforms: &Path) -> Result<i32> {
    // inside the device's data directory, which the phone's processes see
    let work = dev
        .dir
        .join(format!("data/tmp/agent-sim-xctest/{}", std::process::id()));
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;
    let bundle = find_bundle(&unpack(&o.bundle, &work.join("bundle"))?, "xctest")?;
    let app = match &o.app {
        Some(a) => Some(find_bundle(&unpack(a, &work.join("app"))?, "app")?),
        None => None,
    };

    let mut cmd = Command::new("python3");
    cmd.arg(runner_py)
        .env("X86SIM_ROOT", x)
        .env("X86SIM_DEVICE", &dev.dir);
    let mut installed = Vec::new();
    match (o.kind, &app) {
        (Kind::Logic, _) => {
            cmd.arg("--test-bundle").arg(&bundle);
        }
        (Kind::Hosted, Some(app)) => {
            cmd.arg("--app").arg(app).arg("--test-bundle").arg(&bundle);
            installed.push(bundle_id(app)?);
        }
        (Kind::Ui, Some(app)) => {
            let runner = ui_runner(platforms, &bundle, &work)?;
            cmd.arg("--ui-test-runner")
                .arg(&runner)
                .arg("--app")
                .arg(app);
            installed.push(bundle_id(app)?);
            installed.push(bundle_id(&runner)?);
        }
        (_, None) => bail!("hosted and UI tests need the app (--app)"),
    }
    let out = o.out.clone().unwrap_or_else(|| work.join("out"));
    cmd.arg("--platform").arg(platforms).arg("--out").arg(&out);
    cmd.arg("--timeout").arg(o.timeout.to_string());
    for t in &o.only {
        cmd.arg("--only-testing").arg(t);
    }
    if let Some(j) = &o.junit {
        cmd.arg("--junit").arg(j);
    }
    let st = run(&mut cmd)?;
    let code = match &o.expect {
        Some(e) => check_expected(&out.join("results.json"), e, !o.only.is_empty())?,
        None => st.code().unwrap_or(2),
    };

    for bid in &installed {
        uninstall(x, dev, bid);
    }
    // Bazel keeps what a test leaves in TEST_UNDECLARED_OUTPUTS_DIR (bazel-testlogs/<target>/test.outputs)
    if let Some(keep) = std::env::var_os("TEST_UNDECLARED_OUTPUTS_DIR") {
        let _ = copy_dir(&out, &PathBuf::from(keep).join("xctest"));
    }
    if o.out.is_none() {
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(code)
}

/// Compares a run's results (xctest.py's results.json) with the outcomes its tests are meant to have, for test bundles
/// whose failures are deliberate (XCTestDemo's). `expected` lists `<Target>/<Class>/<test> passed|failed|skipped`, one
/// per line (`#` comments); every test it does not list must pass. Returns 0 when every test ran and had its outcome
/// (and, unless a filter chose the tests, every listed test of the run's targets ran), else 1, saying what differed.
pub fn check_expected(results: &Path, expected: &Path, filtered: bool) -> Result<i32> {
    let text = std::fs::read_to_string(expected)
        .with_context(|| format!("reading {}", expected.display()))?;
    let mut want = std::collections::BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut f = line.split_whitespace();
        match (f.next(), f.next(), f.next()) {
            (Some(id), Some(r @ ("passed" | "failed" | "skipped")), None) => {
                want.insert(id.to_string(), r.to_string());
            }
            _ => bail!(
                "{}:{}: expected `<Target>/<Class>/<test> passed|failed|skipped`",
                expected.display(),
                n + 1
            ),
        }
    }
    let run: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(results)
            .with_context(|| format!("reading {}", results.display()))?,
    )
    .with_context(|| format!("parsing {}", results.display()))?;
    let mut wrong = Vec::new();
    let mut checked = 0;
    let targets = run["targets"].as_array().cloned().unwrap_or_default();
    if targets.is_empty() {
        wrong.push("no test target ran".to_string());
    }
    for t in &targets {
        let target = t["name"].as_str().unwrap_or("?");
        let tests = t["tests"].as_array().cloned().unwrap_or_default();
        if tests.is_empty() {
            let why = t["error"].as_str().unwrap_or("no test ran");
            wrong.push(format!("{target}: {why}"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for test in &tests {
            let id = format!("{target}/{}", test["identifier"].as_str().unwrap_or("?"));
            let got = test["result"].as_str().unwrap_or("?");
            let meant = want.get(&id).map(String::as_str).unwrap_or("passed");
            if got != meant {
                wrong.push(format!("{id}: {got}, meant to have {meant}"));
            }
            checked += 1;
            seen.insert(id);
        }
        if !filtered {
            let prefix = format!("{target}/");
            for id in want
                .keys()
                .filter(|k| k.starts_with(&prefix) && !seen.contains(*k))
            {
                wrong.push(format!("{id}: did not run"));
            }
        }
    }
    if wrong.is_empty() {
        println!(
            "agent-sim xctest: all {checked} tests had the outcomes {} lists",
            expected.display()
        );
        return Ok(0);
    }
    for w in &wrong {
        eprintln!("agent-sim xctest: {w}");
    }
    eprintln!(
        "agent-sim xctest: {} of the outcomes differ from {}",
        wrong.len(),
        expected.display()
    );
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::check_expected;
    use std::path::PathBuf;

    fn files(name: &str, results: &str, expected: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("agent-sim-expect-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("results.json"), results).unwrap();
        std::fs::write(dir.join("expected.txt"), expected).unwrap();
        (dir.join("results.json"), dir.join("expected.txt"))
    }

    const RUN: &str = r#"{"targets": [{"name": "T", "result": "failed", "tests": [
        {"identifier": "C/testOk", "result": "passed"},
        {"identifier": "C/testBad", "result": "failed"},
        {"identifier": "C/testSkip", "result": "skipped"}]}]}"#;

    #[test]
    fn deliberate_outcomes_pass() {
        let (r, e) = files(
            "ok",
            RUN,
            "# meant\nT/C/testBad failed\nT/C/testSkip skipped\n",
        );
        assert_eq!(check_expected(&r, &e, false).unwrap(), 0);
    }

    #[test]
    fn an_unexpected_failure_or_a_missing_test_fails() {
        let (r, e) = files("bad", RUN, "T/C/testSkip skipped\n");
        assert_eq!(check_expected(&r, &e, false).unwrap(), 1);
        let (r, e) = files(
            "missing",
            RUN,
            "T/C/testBad failed\nT/C/testSkip skipped\nT/C/testGone failed\n",
        );
        assert_eq!(check_expected(&r, &e, false).unwrap(), 1);
        // a filter chose the tests: a listed test that did not run is not missing
        assert_eq!(check_expected(&r, &e, true).unwrap(), 0);
    }

    #[test]
    fn a_target_that_ran_nothing_fails() {
        let (r, e) = files(
            "empty",
            r#"{"targets": [{"name": "T", "tests": [], "error": "the device did not answer"}]}"#,
            "",
        );
        assert_eq!(check_expected(&r, &e, false).unwrap(), 1);
    }
}
