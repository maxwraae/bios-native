//! `cargo xtask app`: Bios as a macOS application to double-click, in `dist/Bios.app`.
//!
//! An optimised binary in a bundle, signed only with an ad-hoc signature. `cargo xtask release`
//! puts this bundle in the disk image published on GitHub; it reaches other Macs through
//! `install.sh` and the updater, whose downloads are not quarantined, so Gatekeeper lets it open.

use std::path::{Path, PathBuf};
use std::process::Command;

use craft_xtask::runner;

pub const NAME: &str = "Bios";
/// The same id the window gives the system (`APP_ID` in `apps/bios`).
pub const BUNDLE_ID: &str = "com.wraae.bios.native";

/// The bundle's `Info.plist` for a version such as `0.1.0`.
pub fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>{NAME}</string>
  <key>CFBundleName</key>
  <string>{NAME}</string>
  <key>CFBundleExecutable</key>
  <string>bios</string>
  <key>CFBundleIdentifier</key>
  <string>{BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{version}</string>
  <key>CFBundleVersion</key>
  <string>{version}</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.productivity</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright © The Bios contributors. MIT OR Apache-2.0.</string>
</dict>
</plist>
"#
    )
}

/// The workspace's version, from the `[workspace.package]` table of the root manifest.
pub fn workspace_version(manifest: &str) -> Option<String> {
    let table = manifest.split("[workspace.package]").nth(1)?;
    let table = table.split("\n[").next()?;
    let line = table.lines().find(|line| line.trim_start().starts_with("version"))?;
    Some(line.split('"').nth(1)?.to_string())
}

fn io(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("{what} {}: {e}", path.display())
}

pub fn run(root: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("cargo xtask app builds a macOS application; on this system run `cargo build --release -p bios`".to_string());
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| io("reading", &root.join("Cargo.toml"), e))?;
    let version = workspace_version(&manifest).ok_or("the root Cargo.toml has no [workspace.package] version")?;

    let mut build = runner::cargo(root);
    // The command line ships inside the app so that one download is enough to connect an agent
    // (Help > Connect an Agent points at it).
    build.args(["build", "--release", "--package", "bios", "--package", "bios-cli"]);
    let status = build.status().map_err(|e| format!("cargo build could not start: {e}"))?;
    if !status.success() {
        return Err("cargo build --release --package bios --package bios-cli failed".to_string());
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let binary = target.join("release").join("bios");

    let bundle = root.join("dist").join(format!("{NAME}.app"));
    let contents = bundle.join("Contents");
    if bundle.exists() {
        std::fs::remove_dir_all(&bundle).map_err(|e| io("replacing", &bundle, e))?;
    }
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos).map_err(|e| io("making", &macos, e))?;
    std::fs::copy(&binary, macos.join("bios")).map_err(|e| io("copying", &binary, e))?;
    let cli = target.join("release").join("bios-cli");
    std::fs::copy(&cli, macos.join("bios-cli")).map_err(|e| io("copying", &cli, e))?;
    std::fs::write(contents.join("Info.plist"), info_plist(&version)).map_err(|e| io("writing", &contents.join("Info.plist"), e))?;
    std::fs::write(contents.join("PkgInfo"), "APPL????").map_err(|e| io("writing", &contents.join("PkgInfo"), e))?;

    // An ad-hoc signature covers the bundle as a whole, which is what lets it run where it was
    // built. Without one the binary's own linker signature still does; so a failure is said, not fatal.
    match Command::new("codesign").args(["--force", "--sign", "-"]).arg(&bundle).status() {
        Ok(status) if status.success() => {}
        _ => eprintln!("app: the bundle was not signed (codesign failed); it will still run on this machine"),
    }
    println!("app: {} (version {version})", bundle.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{info_plist, workspace_version};

    #[test]
    fn the_version_comes_from_the_workspace_table_and_no_other() {
        let manifest = "[package]\nversion = \"9.9.9\"\n\n[workspace.package]\nedition = \"2024\"\nversion = \"0.3.1\"\n\n[workspace.dependencies]\nserde = { version = \"1\" }\n";
        assert_eq!(workspace_version(manifest).as_deref(), Some("0.3.1"));
        assert_eq!(workspace_version("[package]\nversion = \"1.0.0\"\n"), None);
    }

    #[test]
    fn the_bundle_names_its_executable_and_carries_the_version() {
        let plist = info_plist("0.3.1");
        assert!(plist.contains("<key>CFBundleExecutable</key>\n  <string>bios</string>"));
        assert_eq!(plist.matches("<string>0.3.1</string>").count(), 2);
        assert!(plist.contains("<string>com.wraae.bios.native</string>"));
    }
}
