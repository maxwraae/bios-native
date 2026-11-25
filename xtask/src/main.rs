//! `cargo xtask`: the checks every change passes before it is called done.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod app;
mod layers;
mod release;
mod ship;
mod smoke;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use craft_xtask::assets::{self, Policy};
use craft_xtask::runner::{self, RunOptions, Step, TestMode};

const USAGE: &str = "\
cargo xtask <command>

  ci        fmt --check, clippy -D warnings, test, assets, layers; with BIOS_PRIVATE_FIXTURES
            set, also the tests on answers recorded from private data kept in that folder
  assets    every asset file has a row in ATTRIBUTION.md
  layers    crates depend only on lower layers; only the drawing crates use the UI toolkit
  app       build Bios as a macOS application in dist/Bios.app, for this machine
  ship <host> [--no-check]
            ci, then app, then install it in ~/Applications on another Mac over ssh and open it
  release [--no-check]
            ci, then app, then the release files in dist/ (the disk image the updater and
            install.sh download, and SHA256SUMS.txt); prints the gh command, does not run it
  smoke     the built programs answer for themselves on this machine: the command line, a real
            MCP conversation, the melting temperature, and the window drawing itself";

fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("ci") => root().and_then(|r| ci(&r)),
        Some("assets") => root().and_then(|r| assets_check(&r)),
        Some("layers") => root().and_then(|r| layers::run(&r)),
        Some("app") => root().and_then(|r| app::run(&r)),
        Some("smoke") => root().and_then(|r| smoke::run(&r)),
        Some("release") => {
            let check = !std::env::args().skip(2).any(|a| a == "--no-check");
            root().and_then(|r| release::run_release(&r, check))
        }
        Some("ship") => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            let check = !rest.iter().any(|a| a == "--no-check");
            let host = rest.iter().find(|a| !a.starts_with("--")).map(String::as_str);
            root().and_then(|r| ship::run(&r, host, check, app::BUNDLE_ID, app::NAME))
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> Result<PathBuf, String> {
    runner::root(Path::new(env!("CARGO_MANIFEST_DIR"))).map_err(String::from)
}

/// The folder holding fixtures recorded from private data, in this repository's layout.
const PRIVATE_FIXTURES: &str = "BIOS_PRIVATE_FIXTURES";

pub(crate) fn ci(root: &Path) -> Result<(), String> {
    let mut steps = vec![
        Step::new("fmt", |log| {
            let mut c = runner::cargo(root);
            c.args(["fmt", "--all", "--", "--check"]);
            runner::run(c, log.command("cargo fmt --all -- --check"))
        }),
        Step::new("clippy", |log| {
            let mut c = runner::cargo(root);
            c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
            runner::run(c, log.command("cargo clippy --workspace --all-targets -- -D warnings"))
        }),
        Step::new("test", |log| {
            let (c, label) = runner::nextest_or_test(root, "xtask", TestMode::CargoTest, &[] as &[&str])?;
            runner::run(c, log.command(&label))
        }),
        Step::new("xtask tests", |log| {
            let mut c = runner::cargo(root);
            c.args(["test", "--package", "xtask"]);
            runner::run(c, log.command("cargo test --package xtask"))
        }),
        Step::new("assets", |_| assets_check(root)),
        Step::new("layers", |_| layers::run(root)),
    ];
    // Answers recorded from private data are kept outside the repository. Their tests are ignored
    // and sit in a module named `private`; with the folder named, they run too.
    if std::env::var_os(PRIVATE_FIXTURES).is_some() {
        steps.push(Step::new("private fixtures", |log| {
            let mut c = runner::cargo(root);
            c.args(["test", "--workspace", "--exclude", "xtask", "--", "--ignored", "private::"]);
            runner::run(c, log.command("cargo test --workspace --exclude xtask -- --ignored private::"))
        }));
    }
    runner::run_steps(&mut steps, &RunOptions::default()).map(|_| ()).map_err(String::from)
}

fn assets_check(root: &Path) -> Result<(), String> {
    let report = assets::run(root, &Policy::new("ATTRIBUTION.md")).map_err(|e| e.to_string())?;
    if report.is_ok() {
        println!("assets: {} attributed", report.assets);
        return Ok(());
    }
    Err(report.violations.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))
}
