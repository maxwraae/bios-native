//! `cargo xtask layers`: the crate dependency rule.
//!
//! A crate may depend only on workspace crates in strictly lower layers, and only the crates in
//! [`MAY_DRAW`] may depend on the UI toolkit. This is what keeps the engine usable without a
//! window: by the command line, by MCP and by a server.

use std::path::Path;

use craft_xtask::runner;

/// Lowest first. A new workspace crate is added here; a crate missing from the table fails the check.
const LAYERS: &[(&str, u8)] = &[
    ("bios-core", 0),
    ("bios-layout", 1),
    ("bios-mapper", 1),
    ("bios-project", 2),
    ("bios-sync", 3),
    ("bios-engine", 4),
    ("bios-mcp", 5),
    ("bios-update", 5),
    ("bios-ui", 6),
    ("bios", 7),
    ("bios-cli", 7),
    ("xtask", 7),
];

const TOOLKIT: &[&str] = &["egui", "eframe", "epaint", "emath", "ecolor", "egui-winit", "egui-wgpu", "egui_extras", "winit", "wgpu"];
const MAY_DRAW: &[&str] = &["bios-ui", "bios"];

/// One workspace crate and the names of everything it depends on, dev and build dependencies included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Krate {
    pub name: String,
    pub deps: Vec<String>,
}

fn layer(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

/// Every way `crates` breaks the rule, one line each. Empty when the layering holds.
pub fn check(crates: &[Krate]) -> Vec<String> {
    let mut out = Vec::new();
    for c in crates {
        let Some(from) = layer(&c.name) else {
            out.push(format!("{}: not in the layer table; add it to xtask/src/layers.rs", c.name));
            continue;
        };
        for d in &c.deps {
            if TOOLKIT.contains(&d.as_str()) && !MAY_DRAW.contains(&c.name.as_str()) {
                out.push(format!("{} -> {d}: only {} may depend on the UI toolkit", c.name, MAY_DRAW.join(" and ")));
            }
            if let Some(to) = layer(d)
                && to >= from
            {
                out.push(format!("{} (layer {from}) -> {d} (layer {to}): a crate may depend only on strictly lower layers", c.name));
            }
        }
    }
    out
}

/// The workspace's crates, read from `cargo metadata`.
fn workspace(root: &Path) -> Result<Vec<Krate>, String> {
    let mut cmd = runner::cargo(root);
    cmd.args(["metadata", "--format-version", "1", "--no-deps"]);
    let output = cmd.output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let packages = json.get("packages").and_then(|p| p.as_array()).ok_or("cargo metadata: no packages")?;
    let name = |v: &serde_json::Value| v.get("name").and_then(|n| n.as_str()).map(str::to_owned);
    Ok(packages
        .iter()
        .filter_map(|p| {
            let deps = p.get("dependencies").and_then(|d| d.as_array()).map(|d| d.iter().filter_map(name).collect()).unwrap_or_default();
            Some(Krate { name: name(p)?, deps })
        })
        .collect())
}

pub fn run(root: &Path) -> Result<(), String> {
    let crates = workspace(root)?;
    let violations = check(&crates);
    if violations.is_empty() {
        println!("layers: {} crates, ok", crates.len());
        return Ok(());
    }
    Err(violations.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn krate(name: &str, deps: &[&str]) -> Krate {
        Krate { name: name.to_owned(), deps: deps.iter().map(|d| (*d).to_owned()).collect() }
    }

    #[test]
    fn a_crate_may_use_lower_layers() {
        assert!(check(&[krate("bios-engine", &["bios-core", "bios-project", "serde"])]).is_empty());
    }

    #[test]
    fn a_crate_may_not_use_a_higher_or_equal_layer() {
        assert_eq!(check(&[krate("bios-core", &["bios-layout"])]).len(), 1);
        assert_eq!(check(&[krate("bios", &["bios-cli"])]).len(), 1);
    }

    #[test]
    fn only_the_drawing_crates_may_use_the_toolkit() {
        assert_eq!(check(&[krate("bios-engine", &["egui"])]).len(), 1);
        assert!(check(&[krate("bios-ui", &["egui"]), krate("bios", &["eframe"])]).is_empty());
    }

    #[test]
    fn an_unknown_crate_fails() {
        assert_eq!(check(&[krate("bios-surprise", &[])]).len(), 1);
    }
}
