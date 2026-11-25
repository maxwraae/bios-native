//! `cargo xtask smoke`: the built programs answer for themselves.
//!
//! This is what makes "it works on Windows" a fact rather than a hope. `cargo xtask ci` proves the
//! code; this proves the two programs that are actually published — on the machine they were built
//! for, through the same paths a person and an agent use: the command line, a real MCP
//! conversation over pipes, and the window opening far enough to draw itself.
//!
//! It is written here, in Rust, and not as a shell step in the workflows, for three reasons that
//! each cost a day to learn the other way round:
//!
//! - `Command::output` always waits for the child. PowerShell does not wait for a program with no
//!   console, which `bios.exe` is, so a `--shot` step written in PowerShell would pass before the
//!   picture existed — a test that cannot fail.
//! - An argument given to `Command` arrives byte for byte. The JSON below survives neither
//!   PowerShell 5.1's quoting nor `cmd`'s.
//! - The same check then runs on a Mac, so the Mac is held to it too and this cannot quietly rot
//!   into a Windows-only script.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// A sequence whose answers are known: 24 bases, 62.5% GC, and a melting temperature of 64.7 °C
/// by the nearest-neighbour estimate Bios uses everywhere (SantaLucia 1998, 250 nM, 50 mM Na+).
/// The number is the point of this file: it is the one assertion that would catch a machine
/// computing the maths differently from the Mac the recorded answers came from.
const SEQUENCE: &str = "ATGGTGAGCAAGGGCGAGGAGCTG";
const TM: f64 = 64.7;
const GC: f64 = 62.5;

/// How long the MCP conversation is given before it is called a failure.
const MCP_WAIT: Duration = Duration::from_secs(30);
/// How long the window is given to open, draw and save its picture.
const WINDOW_WAIT: Duration = Duration::from_secs(120);

/// Commands that must be there whatever else changes. A count alone would be a false alarm the
/// next time a command is added, and no check at all would miss an empty registry.
const MUST_HAVE: &[&str] = &["sequence_stats", "read_plasmid", "insert_sequence", "design_primers", "check_for_update", "show_in_bios"];

fn program(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn output(what: &str, command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| format!("{what} could not start: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Reads a child's stdout to the end, or gives up. A child that never closes its output would
/// otherwise hold the whole check open for ever.
fn read_until_eof(child: &mut Child, wait: Duration) -> Result<String, String> {
    let stdout = child.stdout.take().ok_or("the child has no output to read")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            text.push_str(&line);
            text.push('\n');
        }
        let _ = tx.send(text);
    });
    match rx.recv_timeout(wait) {
        Ok(text) => Ok(text),
        Err(_) => {
            let _ = child.kill();
            Err(format!("nothing more was said within {} seconds", wait.as_secs()))
        }
    }
}

fn number(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    value.get(key).and_then(serde_json::Value::as_f64).ok_or_else(|| format!("the answer has no {key}: {value}"))
}

/// Checks the two numbers every surface must agree on.
fn check_stats(what: &str, stats: &serde_json::Value) -> Result<(), String> {
    let length = number(stats, "length")?;
    let gc = number(stats, "gc_percent")?;
    let tm = number(stats, "tm_c")?;
    if length != SEQUENCE.len() as f64 {
        return Err(format!("{what}: {length} bases, not {}", SEQUENCE.len()));
    }
    if (gc - GC).abs() > 0.05 {
        return Err(format!("{what}: {gc}% GC, not {GC}%"));
    }
    if (tm - TM).abs() > 0.05 {
        return Err(format!(
            "{what}: a melting temperature of {tm} °C, not {TM} °C. The maths differs on this machine, which is the one thing that must never differ."
        ));
    }
    println!("smoke: {what}: {length} bases, {gc}% GC, {tm} °C");
    Ok(())
}

/// A real MCP conversation with the published program, over pipes, as an agent has it.
///
/// `crates/bios-mcp/tests/mcp.rs` calls the server in-process; this proves the built executable
/// speaks the protocol, and that what it answers matches what the command line answers.
fn check_mcp(cli: &Path, project: &Path, config: &Path) -> Result<(), String> {
    let mut child = Command::new(cli)
        .arg("mcp")
        .arg(project)
        .env("BIOS_CONFIG_DIR", config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("bios-cli mcp could not start: {e}"))?;

    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "sequence_stats", "arguments": { "sequence": SEQUENCE } }
    });
    let conversation = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"xtask smoke","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        call,
    ];
    {
        let mut stdin = child.stdin.take().ok_or("bios-cli mcp took no input")?;
        for line in conversation {
            writeln!(stdin, "{line}").map_err(|e| format!("writing to bios-cli mcp: {e}"))?;
        }
        // Closed, so the server finishes and we read to the end rather than guessing when to stop.
    }
    let said = read_until_eof(&mut child, MCP_WAIT)?;
    let _ = child.wait();

    let mut tools = 0usize;
    let mut instructions = String::new();
    let mut stats = None;
    for line in said.lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match message.get("id").and_then(serde_json::Value::as_i64) {
            Some(1) => instructions = message.pointer("/result/instructions").and_then(|i| i.as_str()).unwrap_or_default().to_string(),
            Some(2) => tools = message.pointer("/result/tools").and_then(|t| t.as_array()).map_or(0, Vec::len),
            Some(3) => {
                let text = message.pointer("/result/content/0/text").and_then(|t| t.as_str()).unwrap_or_default();
                stats = serde_json::from_str::<serde_json::Value>(text).ok();
            }
            _ => {}
        }
    }

    if tools < 100 {
        return Err(format!("the MCP server offered {tools} tools, which is too few to be the whole registry"));
    }
    // The preamble is what stops an agent computing the maths itself. If it goes missing, an agent
    // reaches for its own primer3 again, which is the thing this all exists to prevent.
    if !instructions.contains("SantaLucia") {
        return Err("the MCP server's instructions no longer say where the melting temperature comes from".to_string());
    }
    let stats = stats.ok_or("the MCP server did not answer sequence_stats")?;
    check_stats("over MCP", &stats)?;
    println!("smoke: MCP: {tools} tools, and the instructions still name the melting-temperature estimate");
    Ok(())
}

/// Opens the window far enough to draw itself and save a picture.
///
/// On a Mac this is the same check `cargo xtask ci` leans on. On Windows it is the only proof that
/// the drawing stack, the fonts and the layout come up at all, so when it cannot be had the
/// workflow keeps the picture as a best effort and says so, rather than reporting a pass.
fn check_window(window: &Path, project: &Path, config: &Path, shot: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(shot);
    let mut child = Command::new(window)
        .arg(project)
        .arg("--after")
        .arg("1")
        .arg("--shot")
        .arg(shot)
        .env("BIOS_CONFIG_DIR", config)
        // Windows runners draw on a software adapter; naming the backend stops wgpu shopping
        // around for an OpenGL driver that is not there.
        .env("WGPU_BACKEND", if cfg!(windows) { "dx12" } else { "metal" })
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("the window could not start: {e}"))?;

    let deadline = std::time::Instant::now() + WINDOW_WAIT;
    loop {
        match child.try_wait().map_err(|e| format!("waiting for the window: {e}"))? {
            Some(status) if status.success() => break,
            Some(status) => return Err(format!("the window stopped without saving a picture ({status})")),
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return Err(format!("the window did not finish within {} seconds", WINDOW_WAIT.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }

    let bytes = std::fs::read(shot).map_err(|e| format!("the window saved no picture at {}: {e}", shot.display()))?;
    // A PNG, and big enough to be a window rather than an empty frame.
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(format!("{} is not a PNG", shot.display()));
    }
    if bytes.len() < 10_000 {
        return Err(format!("{} is only {} bytes, which is too small to be the window", shot.display(), bytes.len()));
    }
    println!("smoke: the window drew itself and saved {} ({} bytes)", shot.display(), bytes.len());
    Ok(())
}

pub fn run(root: &Path) -> Result<(), String> {
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let release = target.join("release");
    let cli = release.join(program("bios-cli"));
    let window = release.join(program("bios"));
    for path in [&cli, &window] {
        if !path.is_file() {
            return Err(format!("{} is not there. Build it first: cargo build --release -p bios -p bios-cli", path.display()));
        }
    }

    // Everything happens in a folder of its own, with the settings folder moved into it, so a
    // check never touches the person's own projects or their list of recent folders.
    let work = target.join("smoke");
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("clearing {}: {e}", work.display()))?;
    }
    let config = work.join("settings");
    std::fs::create_dir_all(&config).map_err(|e| format!("making {}: {e}", config.display()))?;
    let project = work.join("Smoke");

    let made = output("bios-cli new", Command::new(&cli).arg("new").arg(&project).arg("Smoke").env("BIOS_CONFIG_DIR", &config))?;
    if !project.join("bios.json").is_file() {
        return Err(format!("bios-cli new said {} but wrote no project", made.trim()));
    }
    println!("smoke: made a project in {}", project.display());

    let listed = output("bios-cli commands", Command::new(&cli).arg("commands").env("BIOS_CONFIG_DIR", &config))?;
    let count = listed.lines().filter(|line| !line.trim().is_empty()).count();
    if count < 100 {
        return Err(format!("bios-cli lists {count} commands, which is too few to be the whole registry"));
    }
    for name in MUST_HAVE {
        if !listed.contains(name) {
            return Err(format!("bios-cli does not list {name}"));
        }
    }
    println!("smoke: {count} commands, including every one that had to be there");

    let answered = output(
        "bios-cli run sequence_stats",
        Command::new(&cli)
            .arg("run")
            .arg(&project)
            .arg("sequence_stats")
            .arg(format!("{{\"sequence\":\"{SEQUENCE}\"}}"))
            .env("BIOS_CONFIG_DIR", &config),
    )?;
    let stats: serde_json::Value =
        serde_json::from_str(&answered).map_err(|e| format!("sequence_stats did not answer with JSON ({e}): {answered}"))?;
    check_stats("on the command line", &stats)?;

    check_mcp(&cli, &project, &config)?;

    let shot = work.join("window.png");
    match check_window(&window, &project, &config, &shot) {
        Ok(()) => {}
        Err(why) if std::env::var_os("BIOS_SMOKE_PICTURE_OPTIONAL").is_some() => {
            // The workflow sets this on Windows, where a runner may have no surface to draw on.
            // Said loudly and left as a failure of its own step, never folded into a pass.
            eprintln!("smoke: no picture of the window on this machine: {why}");
            eprintln!("smoke: the programs, the command line, MCP and the maths are still proven above.");
        }
        Err(why) => return Err(why),
    }

    println!("smoke: Bios answers for itself on this machine.");
    Ok(())
}
