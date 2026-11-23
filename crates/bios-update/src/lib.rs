//! Bios updating itself from its GitHub releases, with ArtCraft's `craft-update`.
//!
//! This is the [`bios_engine::update::Updater`] the window and `bios-cli` hand the engine at start
//! ([`set_up`]), so that `check_for_update` and `update_bios` work the same from every surface.
//!
//! A release carries one disk image per Mac, `bios-<version>-macos-<arch>.dmg` (`cargo xtask
//! release` makes it), holding `Bios.app`. An update reads the release list, picks the image for
//! this Mac, downloads it only from this repository's own release links, checks it against the
//! SHA-256 GitHub publishes for it, and swaps the new `Bios.app` in where the running one is
//! (`/Applications`, `~/Applications`, anywhere), never somewhere else.
//!
//! One check differs from craft-update's own self-update: that one insists on ArtCraft's Apple
//! Developer ID, and Bios is signed ad hoc (`cargo xtask app`), so it would refuse every Bios. Here
//! the bundle must be Bios (its bundle id) at the release's version, and the image must match the
//! published checksum, fetched over HTTPS from the pinned repository; an ad-hoc signature says
//! nothing about who made a build, so checking it would add nothing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use bios_engine::update::{Check, Installed, Updater};
use craft_update::http::Fetched;
use craft_update::{App, Asset, Http, PackageKind, Paths, Platform, Release, Stop};

/// The repository Bios is published from. Releases are only ever downloaded from it.
pub const REPO: &str = "maxwraae/bios-native";

/// The app's bundle id, as `cargo xtask app` writes it into `Info.plist`.
pub const BUNDLE_ID: &str = "com.wraae.bios.native";

/// This build's version, the workspace's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bios as craft-update knows it: release assets named `bios-…`, the bundle `Bios.app`.
pub static APP: App =
    App { bundle_id_override: Some(BUNDLE_ID), developer: "the Bios contributors", ..App::new("bios", "Bios", "Designing DNA", REPO) };

/// What an installation does about the window that is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// This process is the window: it closes ([`relaunch_pending`]) and opens again
    /// ([`relaunch_after_exit`]).
    ThisWindow,
    /// This process is the command line or an agent's connection: a Bios window that is open is
    /// asked to quit and is opened again.
    RunningWindow,
}

/// Sets up the updater every session of this process uses. Call once, at start.
pub fn set_up(after: After) {
    bios_engine::update::set_updater(Arc::new(GithubUpdater { after }));
}

/// Bios's releases on GitHub.
pub struct GithubUpdater {
    pub after: After,
}

impl Updater for GithubUpdater {
    fn check(&self) -> Result<Check, String> {
        let http = http()?;
        let release = latest(&http)?;
        Ok(summarize(release.as_ref(), &running_version(), installed_bundle(), Platform::current()))
    }

    fn install(&self) -> Result<Installed, String> {
        let http = http()?;
        let release = latest(&http)?.ok_or("no release of Bios has been published yet")?;
        let running = running_version();
        let check = summarize(Some(&release), &running, installed_bundle(), Platform::current());
        if !check.newer {
            return Err(format!("Bios {running} is already the newest version"));
        }
        if let Some(why) = check.cannot_install {
            return Err(why);
        }
        let bundle = installed_bundle()?;
        let asset = Platform::current().and_then(|p| pick(&release, p)).ok_or("the release has no disk image for this Mac")?;
        let placed = install_release(&release, asset, &bundle, &http, &Paths::for_app(&APP)?)?;
        let relaunch = match self.after {
            After::ThisWindow => {
                *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(placed.clone());
                "This Bios window closes and opens again on the new version.".to_string()
            }
            After::RunningWindow => reopen_running_window(&placed),
        };
        Ok(Installed { from: running, to: release.version_label(), location: placed.display().to_string(), relaunch })
    }
}

/// The version this copy counts as: its own, or `BIOS_PRETEND_VERSION` to try an update
/// against a real release (craft-update's `<APP>_PRETEND_VERSION`).
pub fn running_version() -> String {
    craft_update::self_update::current_version(&APP, VERSION).map_or_else(|| VERSION.to_string(), |v| v.to_string())
}

fn http() -> Result<Http, String> {
    // The product and its version, nothing about the person.
    Http::new(&format!("Bios/{VERSION}"))
}

/// Where the release list is read from: GitHub, or with the `local-mirror` feature a loopback
/// server named by `BIOS_UPDATE_API`.
fn api_base() -> String {
    #[cfg(feature = "local-mirror")]
    if let Ok(base) = std::env::var("BIOS_UPDATE_API")
        && craft_update::http::is_loopback(&base)
    {
        return base.trim_end_matches('/').to_string();
    }
    "https://api.github.com".to_string()
}

/// The newest stable release, or `None` when nothing is published (or the repository is not
/// there yet).
fn latest(http: &Http) -> Result<Option<Release>, String> {
    let url = format!("{}/repos/{REPO}/releases?per_page=30", api_base());
    match http.get_api(&url, None)? {
        Fetched::Body { text, .. } => craft_update::parse_github_release_list(&text, REPO, false),
        Fetched::NotFound | Fetched::NotModified => Ok(None),
        Fetched::RateLimited { .. } => Err("GitHub's limit on requests was reached; try again in an hour".into()),
    }
}

/// What a check says, from the newest `release`, the version running, where this copy is
/// installed (or why it is not an installed app) and the machine.
pub fn summarize(release: Option<&Release>, running: &str, bundle: Result<PathBuf, String>, platform: Option<Platform>) -> Check {
    let newer = release
        .and_then(|r| r.version.as_ref())
        .zip(craft_update::parse_version(running).as_ref())
        .is_some_and(|(latest, running)| craft_update::version::offers_update(latest, running));
    let cannot_install = match (bundle, release) {
        (Err(why), _) => Some(why),
        (Ok(_), Some(release)) if platform.and_then(|p| pick(release, p)).is_none() => {
            Some(format!("the release has no disk image for this Mac (it should carry {})", asset_name(&release.version_label(), "<arch>")))
        }
        (Ok(_), _) => None,
    };
    Check {
        current: running.to_string(),
        latest: release.map(Release::version_label),
        newer,
        notes: release.map(|r| r.notes.clone()).unwrap_or_default(),
        page: release.map_or_else(|| APP.releases_url(), |r| r.html_url.clone()),
        cannot_install,
    }
}

/// The name of a release's disk image for one Mac architecture (`aarch64`, `x86_64` or
/// `universal`), as craft-update picks it.
pub fn asset_name(version: &str, arch: &str) -> String {
    format!("bios-{version}-macos-{arch}.dmg")
}

/// The disk image of `release` for `platform`.
pub fn pick(release: &Release, platform: Platform) -> Option<&Asset> {
    craft_update::pick_asset(release, &[APP.id], platform, &[PackageKind::Dmg]).map(|(asset, _)| asset)
}

/// The installed `Bios.app` this program runs from.
pub fn installed_bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not tell where this copy of Bios is ({e})"))?;
    bundle_of(&exe.canonicalize().unwrap_or(exe))
}

/// The `Bios.app` that holds `exe`, or why it is not one.
pub fn bundle_of(exe: &Path) -> Result<PathBuf, String> {
    let not_installed = || format!("this copy runs from {} rather than from an installed Bios.app", exe.display());
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).ok_or_else(not_installed)?;
    if is_bios(bundle) { Ok(bundle.to_path_buf()) } else { Err(format!("{} is not Bios (its bundle id is not {BUNDLE_ID})", bundle.display())) }
}

#[cfg(target_os = "macos")]
fn is_bios(bundle: &Path) -> bool {
    craft_update::detect::macos::read_bundle(&APP, bundle).is_some()
}

#[cfg(not(target_os = "macos"))]
fn is_bios(_bundle: &Path) -> bool {
    false
}

/// Downloads `asset` of `release` and puts the `Bios.app` inside in place of `bundle`. Returns
/// where the new copy is: `bundle` itself.
pub fn install_release(release: &Release, asset: &Asset, bundle: &Path, http: &Http, paths: &Paths) -> Result<PathBuf, String> {
    craft_update::install::check_download_link(&APP, release, asset)?;
    let sha = match &asset.sha256 {
        Some(sha) => sha.clone(),
        None => published_checksum(release, asset, http)?
            .ok_or_else(|| format!("the release publishes no checksum for {}, so it was not installed", asset.name))?,
    };
    let dmg = paths.downloads().join(&asset.name);
    http.download(&asset.url, &dmg, asset.size, &sha, &Stop::default(), &mut |_| {})?;
    let placed = place(&dmg, bundle, release, paths);
    let _ = std::fs::remove_file(&dmg);
    placed
}

/// The asset's line in the release's `SHA256SUMS.txt`, for a release whose list did not carry
/// GitHub's own digest.
fn published_checksum(release: &Release, asset: &Asset, http: &Http) -> Result<Option<String>, String> {
    let Some(sums) = release.sums_asset() else { return Ok(None) };
    let text = http.get_text(&sums.url).map_err(|e| format!("could not fetch the release's checksums ({e})"))?;
    Ok(text.and_then(|text| craft_update::release::parse_sha256sums(&text).into_iter().find(|(_, name)| *name == asset.name).map(|(sha, _)| sha)))
}

#[cfg(target_os = "macos")]
fn place(dmg: &Path, bundle: &Path, release: &Release, paths: &Paths) -> Result<PathBuf, String> {
    use craft_update::install::macos::{CheckTurn, Expect, install_dmg};
    let existing = craft_update::detect::macos::read_bundle(&APP, bundle).ok_or_else(|| format!("{} is not Bios", bundle.display()))?;
    // No Developer ID to ask for (see the crate's notes); the version must be the release's, and
    // the running app is replaced on purpose.
    let expect = Expect { team: None, version: release.version.as_ref(), quit_first: false };
    let stop = Stop::default();
    install_dmg(&APP, dmg, Some(&existing), None, false, paths, &expect, CheckTurn { stop: &stop, phase: &mut |_| {} })
}

#[cfg(not(target_os = "macos"))]
fn place(_dmg: &Path, _bundle: &Path, _release: &Release, _paths: &Paths) -> Result<PathBuf, String> {
    Err("Bios updates itself only on macOS for now".into())
}

/// The new copy waiting for this window to exit.
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Whether an update was installed in this process and the window should close so it can open
/// again on the new version.
pub fn relaunch_pending() -> bool {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner).is_some()
}

/// Called as the window exits: when an update is waiting, starts a small helper that waits for
/// this process to end and opens the new copy.
pub fn relaunch_after_exit() {
    let Some(bundle) = PENDING.lock().unwrap_or_else(PoisonError::into_inner).take() else { return };
    let started = std::process::Command::new("/bin/sh")
        .args(["-c", "while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open \"$2\"", "sh"])
        .arg(std::process::id().to_string())
        .arg(&bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(e) = started {
        eprintln!("bios: could not reopen {} after the update ({e}); open it again by hand", bundle.display());
    }
}

/// Asks AppleScript about the Bios window. `None` when it cannot answer.
fn osascript(script: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/osascript").args(["-e", script]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn window_running() -> bool {
    osascript(&format!("application id \"{BUNDLE_ID}\" is running")).as_deref() == Some("true")
}

/// Quits an open Bios window and opens the new copy at `bundle` in its place. Says what happened.
fn reopen_running_window(bundle: &Path) -> String {
    if !window_running() {
        return "No Bios window was open; the next one opens on the new version.".into();
    }
    let _ = osascript(&format!("tell application id \"{BUNDLE_ID}\" to quit"));
    for _ in 0..80 {
        if !window_running() {
            return match std::process::Command::new("/usr/bin/open").arg(bundle).status() {
                Ok(status) if status.success() => "The open Bios window closed and opened again on the new version.".into(),
                _ => "The open Bios window closed; open Bios again to use the new version.".into(),
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    "The open Bios window did not quit; quit it and open it again to use the new version.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_update::{Arch, Os};

    const ARM: Platform = Platform { os: Os::MacOs, arch: Arch::Arm64 };
    const INTEL: Platform = Platform { os: Os::MacOs, arch: Arch::X64 };
    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// GitHub's release list as the API writes it, with the assets `cargo xtask release` makes.
    fn list(tag: &str, assets: &[&str]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .map(|name| {
                format!(r#"{{"name":"{name}","size":10,"browser_download_url":"https://github.com/{REPO}/releases/download/{tag}/{name}","digest":"sha256:{HASH}"}}"#)
            })
            .collect();
        format!(
            r#"[{{"tag_name":"{tag}","name":"Bios {tag}","draft":false,"prerelease":false,"body":"Faster rows.","html_url":"https://github.com/{REPO}/releases/tag/{tag}","assets":[{}]}},
               {{"tag_name":"v0.0.9","draft":false,"prerelease":false,"assets":[]}},
               {{"tag_name":"v9.0.0","draft":true,"prerelease":false,"assets":[]}}]"#,
            assets.join(",")
        )
    }

    fn release(tag: &str, assets: &[&str]) -> Release {
        craft_update::parse_github_release_list(&list(tag, assets), REPO, false).unwrap().unwrap()
    }

    const ASSETS: &[&str] = &["bios-0.2.0-macos-aarch64.dmg", "SHA256SUMS.txt"];

    #[test]
    fn the_newest_published_release_is_read_with_its_notes_and_checksums() {
        let r = release("v0.2.0", ASSETS);
        assert_eq!(r.version_label(), "0.2.0", "a draft is never offered");
        assert_eq!(r.notes, "Faster rows.");
        assert_eq!(r.assets[0].sha256.as_deref(), Some(HASH));
    }

    #[test]
    fn the_image_xtask_release_names_is_the_one_picked_for_this_mac() {
        let r = release("v0.2.0", &[&asset_name("0.2.0", "aarch64"), "bios-cli-0.2.0-macos-aarch64.zip", "SHA256SUMS.txt"]);
        assert_eq!(pick(&r, ARM).map(|a| a.name.as_str()), Some("bios-0.2.0-macos-aarch64.dmg"));
        assert!(pick(&r, INTEL).is_none(), "an Intel Mac cannot run an Apple silicon build");
        let universal = release("v0.2.0", &[&asset_name("0.2.0", "universal")]);
        assert!(pick(&universal, INTEL).is_some() && pick(&universal, ARM).is_some());
    }

    #[test]
    fn a_newer_release_is_offered_and_an_equal_or_older_one_is_not() {
        let r = release("v0.2.0", ASSETS);
        let bundle = || Ok(PathBuf::from("/Applications/Bios.app"));
        let check = summarize(Some(&r), "0.1.0", bundle(), Some(ARM));
        assert!(check.newer);
        assert_eq!((check.current.as_str(), check.latest.as_deref()), ("0.1.0", Some("0.2.0")));
        assert_eq!(check.cannot_install, None);
        assert_eq!(check.page, format!("https://github.com/{REPO}/releases/tag/v0.2.0"));
        assert!(!summarize(Some(&r), "0.2.0", bundle(), Some(ARM)).newer);
        assert!(!summarize(Some(&r), "0.3.0", bundle(), Some(ARM)).newer);
        // A build from main past 0.2.0 already holds it.
        assert!(!summarize(Some(&r), "0.2.0-dev", bundle(), Some(ARM)).newer);
    }

    #[test]
    fn no_release_is_not_newer_and_points_at_the_releases_page() {
        let check = summarize(None, "0.1.0", Ok(PathBuf::from("/Applications/Bios.app")), Some(ARM));
        assert!(!check.newer && check.latest.is_none());
        assert_eq!(check.page, format!("https://github.com/{REPO}/releases"));
    }

    #[test]
    fn a_copy_that_is_not_an_installed_app_or_has_no_image_says_why_it_cannot_install() {
        let r = release("v0.2.0", ASSETS);
        let dev = summarize(Some(&r), "0.1.0", Err("this copy runs from target/release/bios".into()), Some(ARM));
        assert!(dev.newer);
        assert!(dev.cannot_install.unwrap().contains("target/release"));
        let intel = summarize(Some(&r), "0.1.0", Ok(PathBuf::from("/Applications/Bios.app")), Some(INTEL));
        assert!(intel.cannot_install.unwrap().contains("no disk image"));
    }

    #[test]
    fn only_this_repositorys_own_release_links_are_downloaded() {
        let r = release("v0.2.0", ASSETS);
        let mut asset = r.assets[0].clone();
        assert!(craft_update::install::check_download_link(&APP, &r, &asset).is_ok());
        asset.url = "https://github.com/someone/else/releases/download/v0.2.0/bios-0.2.0-macos-aarch64.dmg".into();
        assert!(craft_update::install::check_download_link(&APP, &r, &asset).is_err());
    }

    #[test]
    fn a_bundle_is_found_from_the_program_inside_it_and_only_when_it_is_bios() {
        let dir = tempfile::tempdir().unwrap();
        let exe = |id: &str, name: &str| {
            let bundle = dir.path().join(name);
            std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
            std::fs::write(
                bundle.join("Contents/Info.plist"),
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleShortVersionString</key><string>0.1.0</string></dict></plist>"#
                ),
            )
            .unwrap();
            (bundle.clone(), bundle.join("Contents/MacOS/bios-cli"))
        };
        let (bundle, cli) = exe(BUNDLE_ID, "Bios.app");
        if cfg!(target_os = "macos") {
            assert_eq!(bundle_of(&cli).unwrap(), bundle);
        }
        let (_, other) = exe("com.example.bios", "BIOS.app");
        assert!(bundle_of(&other).unwrap_err().contains("is not Bios"));
        assert!(bundle_of(Path::new("/work/target/release/bios")).unwrap_err().contains("rather than from an installed Bios.app"));
    }
}
