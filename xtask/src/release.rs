//! `cargo xtask release`: the files of a GitHub release, in `dist/`, and the command that
//! publishes them, printed and not run.
//!
//! The disk image is what Bios's updater (craft-update's macOS path) and `install.sh` look for:
//! `bios-<version>-macos-<arch>.dmg`, with `Bios.app` at its root. `SHA256SUMS.txt` beside it is
//! the checksum the updater falls back on when GitHub's own digest is missing.

use std::path::Path;
use std::process::Command;

/// Where Bios is published.
pub const REPO: &str = "maxwraae/bios-native";

/// The disk image's name for a version and a Mac architecture, as craft-update parses it
/// (`<stem>-<version>-<os>-<arch>.dmg`).
pub fn asset_name(version: &str, arch: &str) -> String {
    format!("bios-{version}-macos-{arch}.dmg")
}

/// The command that publishes the release.
pub fn gh_command(version: &str, files: &[String]) -> String {
    format!("gh release create v{version} {} --repo {REPO} --title \"Bios {version}\" --generate-notes", files.join(" "))
}

fn run(what: &str, command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| format!("{what} could not start: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn run_release(root: &Path, check: bool) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("cargo xtask release makes a macOS disk image; run it on a Mac".to_string());
    }
    if check {
        crate::ci(root)?;
    }
    crate::app::run(root)?;
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("reading Cargo.toml: {e}"))?;
    let version = crate::app::workspace_version(&manifest).ok_or("the root Cargo.toml has no [workspace.package] version")?;
    // The app was just built for this machine, so it runs on this architecture only.
    let arch = std::env::consts::ARCH;
    let dist = root.join("dist");
    let name = asset_name(&version, arch);
    let dmg = dist.join(&name);

    // What the image holds: the app, and a link to /Applications to drag it onto.
    let stage = dist.join("dmg-root");
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| format!("clearing {}: {e}", stage.display()))?;
    }
    std::fs::create_dir_all(&stage).map_err(|e| format!("making {}: {e}", stage.display()))?;
    let app = dist.join(format!("{}.app", crate::app::NAME));
    run("ditto", Command::new("/usr/bin/ditto").arg(&app).arg(stage.join(format!("{}.app", crate::app::NAME))))?;
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

    let sums = run("shasum", Command::new("/usr/bin/shasum").args(["-a", "256", &name]).current_dir(&dist))?;
    std::fs::write(dist.join("SHA256SUMS.txt"), &sums).map_err(|e| format!("writing SHA256SUMS.txt: {e}"))?;

    let files = [format!("dist/{name}"), "dist/SHA256SUMS.txt".to_string()];
    println!("release: {} and dist/SHA256SUMS.txt (Bios {version}, {arch})", dmg.display());
    println!("To publish it (not run here), from {}:\n\n  {}\n", root.display(), gh_command(&version, &files));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{asset_name, gh_command};

    #[test]
    fn the_image_is_named_as_the_updater_and_install_sh_look_for_it() {
        assert_eq!(asset_name("0.1.0", "aarch64"), "bios-0.1.0-macos-aarch64.dmg");
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
