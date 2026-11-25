//! `cargo xtask release`: this machine's file for a GitHub release, in `dist/`, and the command
//! that publishes it, printed and not run.
//!
//! One release carries one file per kind of machine, and each is built on that kind of machine —
//! the two workflows in `.github/workflows/` run this on a Mac and on a PC and put both in the
//! same release. On a Mac the file is the disk image `bios-<version>-macos-<arch>.dmg` with
//! `Bios.app` at its root; on Windows it is `bios-<version>-windows-x64-portable.zip` with the
//! two programs at its root. `SHA256SUMS.txt` beside it holds a line per file, which is the
//! checksum `install.sh`, `install.ps1` and the updater fall back on when GitHub's own digest is
//! missing; the publishing job joins the two platforms' files into one.

use std::path::Path;
/// Only the two systems Bios is packaged for shell out to anything.
#[cfg(any(target_os = "macos", windows))]
use std::process::Command;

use sha2::{Digest, Sha256};

/// Where Bios is published.
pub const REPO: &str = "maxwraae/bios-native";

/// The disk image's name for a version and a Mac architecture, as craft-update parses it
/// (`<stem>-<version>-<os>-<arch>.dmg`).
pub fn mac_asset_name(version: &str, arch: &str) -> String {
    format!("bios-{version}-macos-{arch}.dmg")
}

/// The Windows zip's name for a version.
///
/// Every part of this is read by craft-update's `parse_asset_name` and cannot be spelled to
/// taste: the suffix must be `-portable.zip`, which is `PackageKind::PortableZip`, and the
/// architecture must be the token `x64`, not `x86_64`. A name it cannot parse is not an error —
/// `pick_asset` simply finds nothing and an installed copy is told there is no version for its
/// machine. One x64 build serves PCs on Arm too: craft-update maps them to the same token.
pub fn windows_asset_name(version: &str) -> String {
    format!("bios-{version}-windows-x64-portable.zip")
}

/// The name of the one file this machine publishes.
///
/// `cfg!` and not `#[cfg]` on purpose: both names are then compiled, and tested, everywhere.
/// They are the contract `install.sh`, `install.ps1`, `bios-update` and craft-update all read, so
/// a change to either of them should break a test on whichever machine a person happens to be at,
/// not only on the one it belongs to.
pub fn asset_name(version: &str) -> String {
    if cfg!(windows) {
        windows_asset_name(version)
    } else {
        // The app was just built for this machine, so it runs on this architecture only.
        mac_asset_name(version, std::env::consts::ARCH)
    }
}

/// `SHA256SUMS.txt`'s text for files in `dist`: two spaces and lower-case hex, the one shape
/// `shasum`, `install.sh`'s `grep`, `install.ps1` and craft-update's parser all agree on. Hashed
/// here rather than by a system tool so that both platforms write the same bytes and the
/// publishing job can join the two files by concatenating them.
fn sums(dist: &Path, names: &[&str]) -> Result<String, String> {
    use std::fmt::Write as _;

    let mut text = String::new();
    for name in names {
        let path = dist.join(name);
        let bytes = std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        let digest = Sha256::digest(&bytes);
        let mut hex = String::with_capacity(64);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        let _ = writeln!(text, "{hex}  {name}");
    }
    Ok(text)
}

/// The command that publishes the release.
pub fn gh_command(version: &str, files: &[String]) -> String {
    format!("gh release create v{version} {} --repo {REPO} --title \"Bios {version}\" --generate-notes", files.join(" "))
}

#[cfg(any(target_os = "macos", windows))]
fn run(what: &str, command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| format!("{what} could not start: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn run_release(root: &Path, check: bool) -> Result<(), String> {
    if check {
        crate::ci(root)?;
    }
    let built = crate::app::build(root)?;
    let version = &built.version;
    let dist = root.join("dist");
    let name = package(&dist, &built)?;

    let text = sums(&dist, &[&name])?;
    std::fs::write(dist.join("SHA256SUMS.txt"), &text).map_err(|e| format!("writing SHA256SUMS.txt: {e}"))?;

    let files = [format!("dist/{name}"), "dist/SHA256SUMS.txt".to_string()];
    println!("release: dist/{name} and dist/SHA256SUMS.txt (Bios {version})");
    println!("To publish it (not run here), from {}:\n\n  {}\n", root.display(), gh_command(version, &files));
    Ok(())
}

/// Wraps what `cargo xtask app` built into the one file this kind of machine publishes, and
/// returns its name inside `dist`.
#[cfg(target_os = "macos")]
fn package(dist: &Path, built: &crate::app::Built) -> Result<String, String> {
    let name = asset_name(&built.version);
    let dmg = dist.join(&name);

    // What the image holds: the app, and a link to /Applications to drag it onto.
    let stage = dist.join("dmg-root");
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| format!("clearing {}: {e}", stage.display()))?;
    }
    std::fs::create_dir_all(&stage).map_err(|e| format!("making {}: {e}", stage.display()))?;
    run("ditto", Command::new("/usr/bin/ditto").arg(&built.dir).arg(stage.join(format!("{}.app", crate::app::NAME))))?;
    run("ln", Command::new("/bin/ln").args(["-s", "/Applications"]).arg(stage.join("Applications")))?;
    let _ = std::fs::remove_file(&dmg);
    run(
        "hdiutil create",
        Command::new("/usr/bin/hdiutil")
            .args(["create", "-quiet", "-volname", crate::app::NAME, "-fs", "HFS+", "-format", "UDZO", "-srcfolder"])
            .arg(&stage)
            .arg(&dmg),
    )?;
    std::fs::remove_dir_all(&stage).map_err(|e| format!("clearing {}: {e}", stage.display()))?;
    Ok(name)
}

/// The portable zip, with `bios.exe` and `bios-cli.exe` at its root.
///
/// Zipped with Windows's own `tar`, which has written zip files since Windows 10, so nothing has
/// to be installed to make a release. The two programs are named rather than passed as `.`,
/// because `tar -C dir -cf out.zip .` writes them as `./bios.exe` and craft-update's portable
/// installer wants the executable at the root or inside exactly one folder.
#[cfg(windows)]
fn package(dist: &Path, built: &crate::app::Built) -> Result<String, String> {
    let name = asset_name(&built.version);
    let zip = dist.join(&name);
    let _ = std::fs::remove_file(&zip);
    let tar =
        std::env::var_os("SystemRoot").map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from).join(r"System32\tar.exe");
    run("tar", Command::new(&tar).arg("-a").arg("-c").arg("-f").arg(&zip).arg("-C").arg(&built.dir).arg("bios.exe").arg("bios-cli.exe"))?;
    Ok(name)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn package(_dist: &Path, _built: &crate::app::Built) -> Result<String, String> {
    Err("cargo xtask release publishes Bios for macOS or Windows; run it on one of those".to_string())
}

#[cfg(test)]
mod tests {
    use super::{gh_command, mac_asset_name, sums, windows_asset_name};

    #[test]
    fn the_image_is_named_as_the_updater_and_install_sh_look_for_it() {
        assert_eq!(mac_asset_name("0.1.0", "aarch64"), "bios-0.1.0-macos-aarch64.dmg");
    }

    /// The exact spelling craft-update's `parse_asset_name` accepts. If this drifts, an installed
    /// copy is quietly told there is no version for its machine, so the string is pinned here, in
    /// `bios-update`, in `install.ps1` and in `install.sh`'s pattern.
    #[test]
    fn the_windows_zip_is_named_as_craft_update_parses_it() {
        assert_eq!(windows_asset_name("0.1.4"), "bios-0.1.4-windows-x64-portable.zip");
        let name = windows_asset_name("0.1.4");
        assert!(name.ends_with("-portable.zip"), "the suffix is PackageKind::PortableZip's");
        assert!(name.contains("-windows-x64-"), "the architecture token is x64, not x86_64");
    }

    #[test]
    fn checksums_are_two_spaces_and_lower_case_hex() {
        let dir = std::env::temp_dir().join(format!("bios-sums-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("a.txt"), b"abc");
        let text = sums(&dir, &["a.txt"]).unwrap_or_default();
        // The SHA-256 of "abc", as every other tool prints it.
        assert_eq!(text, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.txt\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_printed_command_tags_the_version_and_names_the_repository() {
        let line = gh_command("0.1.0", &["dist/bios-0.1.0-macos-aarch64.dmg".into(), "dist/SHA256SUMS.txt".into()]);
        assert_eq!(
            line,
            "gh release create v0.1.0 dist/bios-0.1.0-macos-aarch64.dmg dist/SHA256SUMS.txt --repo maxwraae/bios-native --title \"Bios 0.1.0\" --generate-notes"
        );
    }
}
