//! Turning what the user has (an .ipa built for devices, or a simulator .app) into an .app x86Sim installs.
//!
//! A device build runs in the simulator once its binaries say so: every Mach-O's LC_BUILD_VERSION platform goes from
//! iOS (2) to iOS simulator (7), and Info.plist names the simulator as the platform. The code is the same arm64 code
//! (x86Sim translates it either way). App Store downloads are encrypted (FairPlay) and cannot run anywhere else:
//! those are refused.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct App {
    pub path: PathBuf,
    pub bundle_id: String,
}

const LC_BUILD_VERSION: u32 = 0x32;
const LC_VERSION_MIN_IPHONEOS: u32 = 0x25;
const LC_ENCRYPTION_INFO_64: u32 = 0x2c;
const PLATFORM_IOS: u32 = 2;
const PLATFORM_IOSSIMULATOR: u32 = 7;

#[derive(Debug, PartialEq)]
pub enum Retag {
    NotMachO,
    AlreadySimulator,
    Retagged,
}

fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u32be(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Retags one 64-bit Mach-O image in place (at `base` in the file); refuses encrypted ones.
fn retag_image(b: &mut [u8], base: usize, name: &str) -> Result<Retag> {
    if u32le(b, base) != Some(0xFEED_FACF) {
        return Ok(Retag::NotMachO);
    }
    let ncmds = u32le(b, base + 16).context("truncated Mach-O")? as usize;
    let mut off = base + 32;
    let mut result = Retag::AlreadySimulator;
    let mut saw_build = false;
    for _ in 0..ncmds {
        let cmd = u32le(b, off).context("truncated Mach-O")?;
        let size = u32le(b, off + 4).context("truncated Mach-O")? as usize;
        if size < 8 || off + size > b.len() {
            bail!("{name}: malformed load command");
        }
        match cmd {
            LC_ENCRYPTION_INFO_64 if u32le(b, off + 16).unwrap_or(0) != 0 => bail!(
                "{name} is encrypted (an App Store download): use an unencrypted build (Xcode's Ad Hoc, Development or \
                 Enterprise export, or a simulator build)"
            ),
            LC_BUILD_VERSION => {
                saw_build = true;
                let platform = u32le(b, off + 8).unwrap_or(0);
                if platform == PLATFORM_IOS {
                    b[off + 8..off + 12].copy_from_slice(&PLATFORM_IOSSIMULATOR.to_le_bytes());
                    result = Retag::Retagged;
                } else if platform != PLATFORM_IOSSIMULATOR {
                    bail!("{name} is built for platform {platform}, not iOS");
                }
            }
            LC_VERSION_MIN_IPHONEOS if !saw_build => bail!(
                "{name} predates LC_BUILD_VERSION (built for iOS 11 or older); rebuild it with a current Xcode"
            ),
            _ => {}
        }
        off += size;
    }
    Ok(result)
}

/// Retags a Mach-O file (thin, or every 64-bit slice of a fat one).
pub fn retag(bytes: &mut [u8], name: &str) -> Result<Retag> {
    match u32be(bytes, 0) {
        Some(0xCAFE_BABE) => {
            let n = u32be(bytes, 4).context("truncated fat header")? as usize;
            let mut any = Retag::NotMachO;
            for i in 0..n {
                let o = u32be(bytes, 8 + i * 20 + 8).context("truncated fat header")? as usize;
                match retag_image(bytes, o, name)? {
                    Retag::Retagged => any = Retag::Retagged,
                    Retag::AlreadySimulator if any == Retag::NotMachO => {
                        any = Retag::AlreadySimulator
                    }
                    _ => {}
                }
            }
            Ok(any)
        }
        _ => retag_image(bytes, 0, name),
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)?.flatten() {
        let t = e.file_type()?;
        if t.is_dir() {
            walk(&e.path(), out)?;
        } else if t.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}

fn bundle_id(app: &Path) -> Result<String> {
    let v = plist::Value::from_file(app.join("Info.plist"))
        .with_context(|| format!("reading {}/Info.plist", app.display()))?;
    v.as_dictionary()
        .and_then(|d| d.get("CFBundleIdentifier"))
        .and_then(|s| s.as_string())
        .map(str::to_string)
        .context("Info.plist has no CFBundleIdentifier")
}

/// Info.plist (of the app and of each extension) names the simulator as its platform.
fn retag_plist(path: &Path) -> Result<()> {
    let mut v = plist::Value::from_file(path)?;
    let Some(d) = v.as_dictionary_mut() else {
        return Ok(());
    };
    let mut changed = false;
    if let Some(plist::Value::Array(a)) = d.get_mut("CFBundleSupportedPlatforms")
        && a.iter().any(|x| x.as_string() == Some("iPhoneOS"))
    {
        *a = vec!["iPhoneSimulator".into()];
        changed = true;
    }
    if d.get("DTPlatformName").and_then(|x| x.as_string()) == Some("iphoneos") {
        d.insert("DTPlatformName".into(), "iphonesimulator".into());
        changed = true;
    }
    if changed {
        v.to_file_xml(path)?;
    }
    Ok(())
}

/// Makes an .app the simulator runs from `path` (.app directory, or .ipa).
pub fn prepare(path: &Path) -> Result<App> {
    let path =
        std::fs::canonicalize(path).with_context(|| format!("{} not found", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let app = match ext.as_str() {
        "app" => {
            // retag a copy, so a device .app the user points at is left as it was
            let work = crate::phone::home()?.join("installs");
            std::fs::create_dir_all(&work)?;
            let dst = work.join(path.file_name().unwrap());
            let _ = std::fs::remove_dir_all(&dst);
            let ok = Command::new("cp").arg("-a").arg(&path).arg(&dst).status()?;
            if !ok.success() {
                bail!("copying {}", path.display());
            }
            dst
        }
        "ipa" => {
            let work = crate::phone::home()?
                .join("installs")
                .join(path.file_stem().unwrap());
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work)?;
            let ok = Command::new("python3")
                .args(["-I", "-m", "zipfile", "-e"])
                .arg(&path)
                .arg(&work)
                .status()
                .context("unzipping the .ipa (python3)")?;
            if !ok.success() {
                bail!("{} is not a readable .ipa (zip)", path.display());
            }
            let payload = work.join("Payload");
            std::fs::read_dir(&payload)
                .with_context(|| format!("{} has no Payload folder", path.display()))?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .context("the .ipa's Payload has no .app")?
        }
        _ => bail!("install takes an .app folder or an .ipa file"),
    };
    let mut files = Vec::new();
    walk(&app, &mut files)?;
    let mut retagged = 0;
    for f in &files {
        if f.file_name().is_some_and(|n| n == "Info.plist") {
            retag_plist(f).with_context(|| format!("updating {}", f.display()))?;
            continue;
        }
        let Ok(meta) = std::fs::metadata(f) else {
            continue;
        };
        if meta.len() < 32 {
            continue;
        }
        let mut b = std::fs::read(f)?;
        let name = f.strip_prefix(&app).unwrap_or(f).display().to_string();
        if retag(&mut b, &name)? == Retag::Retagged {
            std::fs::write(f, &b)?;
            retagged += 1;
        }
    }
    if retagged > 0 {
        eprintln!("prepared a device build for the simulator ({retagged} binaries)");
    }
    let bundle_id = bundle_id(&app)?;
    Ok(App {
        path: app,
        bundle_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macho(cmds: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes());
        b[16..20].copy_from_slice(&(cmds.len() as u32).to_le_bytes());
        for (cmd, body) in cmds {
            b.extend_from_slice(&cmd.to_le_bytes());
            b.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
            b.extend_from_slice(body);
        }
        b
    }
    fn build_version(platform: u32) -> Vec<u8> {
        let mut v = platform.to_le_bytes().to_vec();
        v.extend_from_slice(&[0u8; 12]);
        v
    }

    #[test]
    fn device_binaries_become_simulator_ones() {
        let mut b = macho(&[(LC_BUILD_VERSION, build_version(PLATFORM_IOS))]);
        assert_eq!(retag(&mut b, "x").unwrap(), Retag::Retagged);
        assert_eq!(u32le(&b, 40), Some(PLATFORM_IOSSIMULATOR));
        assert_eq!(retag(&mut b, "x").unwrap(), Retag::AlreadySimulator);
    }

    #[test]
    fn encrypted_binaries_are_refused() {
        let mut enc = vec![0u8; 12];
        enc[8..12].copy_from_slice(&1u32.to_le_bytes());
        let mut b = macho(&[
            (LC_BUILD_VERSION, build_version(PLATFORM_IOS)),
            (LC_ENCRYPTION_INFO_64, enc),
        ]);
        assert!(retag(&mut b, "x").is_err());
    }

    #[test]
    fn other_files_are_left_alone() {
        let mut b = b"not a mach-o file at all, just text".to_vec();
        assert_eq!(retag(&mut b, "x").unwrap(), Retag::NotMachO);
    }
}
