//! An agent connected to a folder of projects: `bios-cli mcp <folder>` on a folder whose subfolders are projects.

use std::path::Path;

use bios_mcp::BiosTools;
use bios_project::Project;
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// Sends `calls` as one agent connection to the folder `root` and gives back the replies,
/// starting with `initialize`'s.
fn session(root: &Path, calls: &[Value]) -> Vec<Value> {
    let mut lines = vec![
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    lines.extend(calls.iter().cloned());
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut output = Vec::new();
    bios_mcp::serve(BiosTools::for_agent_in_home(root), std::io::Cursor::new(input), &mut output).unwrap();
    String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn calls(root: &Path, calls: &[(&str, Value)]) -> Vec<Value> {
    let lines: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(
            |(i, (name, arguments))| json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}),
        )
        .collect();
    session(root, &lines).into_iter().skip(1).map(|r| r["result"].clone()).collect()
}

fn body(result: &Value) -> Value {
    assert_ne!(result["isError"], json!(true), "{}", result["content"][0]["text"]);
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn error(result: &Value) -> String {
    assert_eq!(result["isError"], json!(true), "expected an error, got {result}");
    result["content"][0]["text"].as_str().unwrap().to_string()
}

/// A folder holding the projects "Lab" (with wrap_demo) and "Cloning" (with pUC19), and a
/// plain folder that is not a project.
fn home() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    Project::create(&dir.path().join("Cloning"), "Cloning").unwrap();
    std::fs::create_dir(dir.path().join("not a project")).unwrap();
    let replies = calls(
        dir.path(),
        &[
            ("import_plasmid", json!({"project": "Lab", "file": fixture("wrap_demo.gb")})),
            ("import_plasmid", json!({"project": "Cloning", "file": fixture("pUC19.gb")})),
        ],
    );
    for r in &replies {
        body(r);
    }
    dir
}

#[test]
fn the_tools_are_the_commands_with_a_project_plus_listing_and_making_projects() {
    let dir = home();
    let replies = session(dir.path(), &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})]);
    assert!(replies[0]["result"]["instructions"].as_str().unwrap().contains("list_projects"));
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(&names[..6], ["list_projects", "open_project", "create_project", "rename_project", "delete_project", "remove_recent_project"]);
    assert_eq!(names.len(), bios_engine::registry().count() + 6);
    let read = tools.iter().find(|t| t["name"] == "read_plasmid").unwrap();
    assert_eq!(read["inputSchema"]["properties"]["project"]["type"], "string");
    assert!(read["inputSchema"]["properties"]["plasmid"].is_object());
}

#[test]
fn list_projects_shows_only_projects_with_their_plasmid_counts() {
    let dir = home();
    let listed = body(&calls(dir.path(), &[("list_projects", json!({}))])[0]);
    let rows: Vec<(String, String, u64)> = listed["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["project"].as_str().unwrap().into(), r["name"].as_str().unwrap().into(), r["plasmids"].as_u64().unwrap()))
        .collect();
    assert_eq!(rows, [("Cloning".into(), "Cloning".into(), 1), ("Lab".into(), "Lab".into(), 1)]);
    assert_eq!(listed["projects"][1]["path"], dir.path().join("Lab").display().to_string());
}

#[test]
fn reads_and_edits_land_in_the_project_named() {
    let dir = home();
    let read = body(&calls(dir.path(), &[("read_plasmid", json!({"project": "Lab", "plasmid": "wrap_demo", "genbank": false}))])[0]);
    let version = read["version"].as_str().unwrap().to_string();
    let edited = calls(
        dir.path(),
        &[
            ("insert_sequence", json!({"project": "Lab", "plasmid": "wrap_demo", "version": version, "position": 0, "sequence": "GAATTC"})),
            ("read_plasmid", json!({"project": "lab", "plasmid": "wrap_demo", "genbank": false})),
            ("plasmid_history", json!({"project": "Lab", "plasmid": "wrap_demo"})),
        ],
    );
    body(&edited[0]);
    assert_eq!(body(&edited[1])["length"], json!(806));
    let history = body(&edited[2]);
    let last = history["history"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["by"].as_str(), last["command"].as_str()), (Some("agent"), Some("insert_sequence")), "{last}");
    // The other project is untouched.
    let other = body(&calls(dir.path(), &[("list_plasmids", json!({"project": "Cloning"}))])[0]);
    assert!(other.to_string().contains("pUC19"));
    assert!(!other.to_string().contains("wrap_demo"));
}

#[test]
fn a_plasmid_written_as_project_slash_path_needs_no_project() {
    let dir = home();
    let read = body(&calls(dir.path(), &[("read_plasmid", json!({"plasmid": "Cloning/pUC19", "genbank": false}))])[0]);
    assert_eq!(read["length"], json!(2686));
}

#[test]
fn leaving_out_the_project_with_several_names_them() {
    let dir = home();
    let message = error(&calls(dir.path(), &[("list_plasmids", json!({}))])[0]);
    assert!(message.contains("Cloning") && message.contains("Lab"), "{message}");
    assert!(!message.contains("not a project"), "{message}");
}

#[test]
fn an_unknown_project_is_refused_and_the_real_ones_are_named() {
    let dir = home();
    let message = error(&calls(dir.path(), &[("list_plasmids", json!({"project": "Nope"}))])[0]);
    assert!(message.contains("Nope") && message.contains("Cloning, Lab"), "{message}");
}

#[test]
fn a_second_plasmid_is_looked_for_in_the_same_project_only() {
    let dir = home();
    let read = body(&calls(dir.path(), &[("read_plasmid", json!({"project": "Lab", "plasmid": "wrap_demo", "genbank": false}))])[0]);
    let paste = calls(
        dir.path(),
        &[(
            "paste_region",
            json!({"project": "Lab", "plasmid": "wrap_demo", "version": read["version"], "position": 0, "from_plasmid": "pUC19", "from_start": 1, "from_end": 10}),
        )],
    );
    error(&paste[0]);
}

#[test]
fn create_project_makes_a_project_that_can_be_worked_in_at_once() {
    let dir = home();
    let replies = calls(
        dir.path(),
        &[
            ("create_project", json!({"name": "AAV"})),
            ("create_plasmid", json!({"project": "AAV", "name": "pTest", "sequence": "ATGAAATAG"})),
            ("list_projects", json!({})),
        ],
    );
    body(&replies[0]);
    body(&replies[1]);
    assert!(body(&replies[2])["projects"].to_string().contains("\"AAV\""));
    assert!(Project::open(&dir.path().join("AAV")).is_ok());
}

#[test]
fn create_project_refuses_names_that_are_not_folder_names_and_opens_one_already_there() {
    let dir = home();
    let replies = calls(
        dir.path(),
        &[("create_project", json!({"name": "../escape"})), ("create_project", json!({"name": " "})), ("create_project", json!({"name": "Lab"}))],
    );
    error(&replies[0]);
    error(&replies[1]);
    assert!(!dir.path().parent().unwrap().join("escape").exists());
    // A folder that is already there is the person's own: opened where it is, nothing made.
    let lab = body(&replies[2]);
    assert_eq!((lab["project"].as_str(), lab["opened_existing"].as_bool()), (Some("Lab"), Some(true)));
}

#[test]
fn a_project_is_renamed_and_deleting_needs_the_persons_yes() {
    let dir = home();
    let replies = calls(
        dir.path(),
        &[
            ("rename_project", json!({"project": "Lab", "new_name": "Bench"})),
            ("read_plasmid", json!({"project": "Bench", "plasmid": "wrap_demo", "genbank": false})),
            ("delete_project", json!({"project": "Bench", "confirm": false})),
            ("list_projects", json!({})),
        ],
    );
    assert_eq!(body(&replies[0])["project"], "Bench");
    assert!(dir.path().join("Bench/bios.json").is_file());
    body(&replies[1]);
    assert!(error(&replies[2]).contains("confirm"));
    assert_eq!(body(&replies[3])["projects"].as_array().unwrap().len(), 2);
    assert!(dir.path().join("Bench").is_dir());
}
