//! `bios-cli mcp` with no folder: an agent working in the folders opened in Bios, and any folder
//! it is given by path.

use std::fs;
use std::path::{Path, PathBuf};

use bios_mcp::BiosTools;
use bios_project::recents::Recents;
use bios_project::window::{self, Front, ShownProject};
use serde_json::{Value, json};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name)
}

/// One agent connection over the list kept in `file`, making `calls`; the results, in order.
fn calls(file: &Path, calls: &[(&str, Value)]) -> Vec<Value> {
    let mut lines = vec![
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    for (i, (name, arguments)) in calls.iter().enumerate() {
        lines.push(json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));
    }
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut output = Vec::new();
    bios_mcp::serve(BiosTools::for_agent_in_recents(Recents::at(file)), std::io::Cursor::new(input), &mut output).unwrap();
    String::from_utf8(output).unwrap().lines().skip(1).map(|l| serde_json::from_str::<Value>(l).unwrap()["result"].clone()).collect()
}

fn body(result: &Value) -> Value {
    assert_ne!(result["isError"], json!(true), "{}", result["content"][0]["text"]);
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn error(result: &Value) -> String {
    assert_eq!(result["isError"], json!(true), "expected an error, got {result}");
    result["content"][0]["text"].as_str().unwrap().to_string()
}

/// A scratch place: the list's file, and a plain folder of GenBank files as Downloads holds them.
fn place() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let plain = base.join("Downloads");
    fs::create_dir(&plain).unwrap();
    fs::copy(fixture("wrap_demo.gb"), plain.join("wrap_demo.gb")).unwrap();
    fs::copy(fixture("pUC19.gb"), plain.join("pUC19.gb")).unwrap();
    (dir, base.join("settings/recents.json"), plain)
}

#[test]
fn a_folder_given_by_path_is_opened_without_being_written_to_and_joins_the_list() {
    let (_dir, file, plain) = place();
    let path = plain.display().to_string();
    let replies = calls(
        &file,
        &[
            ("list_projects", json!({})),
            ("list_plasmids", json!({})),
            ("open_project", json!({"path": path})),
            ("read_plasmid", json!({"project": path, "plasmid": "pUC19", "genbank": false})),
            ("list_projects", json!({})),
            ("read_plasmid", json!({"project": "Downloads", "plasmid": "wrap_demo", "genbank": false})),
        ],
    );
    assert_eq!(body(&replies[0])["projects"], json!([]));
    assert!(error(&replies[1]).contains("open_project"));
    let opened = body(&replies[2]);
    assert_eq!(
        (opened["project"].as_str(), opened["plasmids"].as_u64(), opened["is_project_yet"].as_bool()),
        (Some("Downloads"), Some(2), Some(false))
    );
    assert_eq!(body(&replies[3])["length"], 2686);
    let listed = body(&replies[4]);
    assert_eq!(listed["projects"], json!([{"project": "Downloads", "name": "Downloads", "path": path, "plasmids": 2}]));
    assert_eq!(body(&replies[5])["length"], 800);
    assert_eq!(fs::read_dir(&plain).unwrap().count(), 2, "reading wrote into the folder");
    // The window and every other agent see the same list.
    assert_eq!(Recents::at(&file).folders(), [plain]);
}

#[test]
fn the_first_edit_by_path_makes_the_folder_a_project() {
    let (_dir, file, plain) = place();
    let path = plain.display().to_string();
    let read = body(&calls(&file, &[("read_plasmid", json!({"project": path, "plasmid": "wrap_demo", "genbank": false}))])[0]);
    let edit = calls(
        &file,
        &[("insert_sequence", json!({"project": path, "plasmid": "wrap_demo", "version": read["version"], "position": 0, "sequence": "GG"}))],
    );
    body(&edit[0]);
    assert!(plain.join("bios.json").is_file());
    assert_eq!(fs::read(plain.join("pUC19.gb")).unwrap(), fs::read(fixture("pUC19.gb")).unwrap());
}

#[test]
fn create_rename_and_remove_keep_the_list_in_step_and_never_delete() {
    let (dir, file, plain) = place();
    let base = dir.path().canonicalize().unwrap();
    let replies = calls(
        &file,
        &[
            ("create_project", json!({"name": "AAV", "path": base.display().to_string()})),
            ("create_plasmid", json!({"project": "AAV", "name": "pTest", "sequence": "ATGAAATAG"})),
            ("open_project", json!({"path": plain.display().to_string()})),
            ("rename_project", json!({"project": "Downloads", "new_name": "Maps"})),
            ("list_projects", json!({})),
            ("remove_recent_project", json!({"project": "Maps"})),
            ("list_projects", json!({})),
            ("create_project", json!({"name": "x", "path": "relative/place"})),
            ("open_project", json!({"path": "not/absolute"})),
        ],
    );
    assert_eq!(body(&replies[0])["path"], base.join("AAV").display().to_string());
    body(&replies[1]);
    body(&replies[2]);
    assert_eq!(body(&replies[3])["path"], base.join("Maps").display().to_string());
    assert!(!base.join("Maps/bios.json").exists(), "renaming a plain folder does not make it a project");
    let names = |v: &Value| v["projects"].as_array().unwrap().iter().map(|r| r["project"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&body(&replies[4])), ["Maps", "AAV"]);
    body(&replies[5]);
    assert_eq!(names(&body(&replies[6])), ["AAV"]);
    assert!(base.join("Maps/wrap_demo.gb").is_file(), "removing from the list leaves the folder");
    error(&replies[7]);
    error(&replies[8]);
}

#[test]
fn two_folders_with_one_name_are_told_apart_by_path() {
    let (dir, file, plain) = place();
    let other = dir.path().canonicalize().unwrap().join("elsewhere/Downloads");
    fs::create_dir_all(&other).unwrap();
    let replies = calls(
        &file,
        &[
            ("open_project", json!({"path": plain.display().to_string()})),
            ("open_project", json!({"path": other.display().to_string()})),
            ("list_plasmids", json!({"project": "Downloads"})),
            ("delete_project", json!({"project": other.display().to_string(), "confirm": false})),
        ],
    );
    let message = error(&replies[2]);
    assert!(message.contains(&other.display().to_string()) && message.contains(&plain.display().to_string()), "{message}");
    assert!(error(&replies[3]).contains("confirm"));
}

/// Checking for an update is about Bios, not a project: it takes no `project`, and runs whether no
/// folder or several are open. This build has no updater set up, so the check says that.
#[test]
fn the_update_commands_take_no_project() {
    let (_dir, file, plain) = place();
    let second = plain.parent().unwrap().join("Other");
    fs::create_dir(&second).unwrap();
    let replies = calls(
        &file,
        &[
            ("check_for_update", json!({})),
            ("open_project", json!({"path": plain.display().to_string()})),
            ("open_project", json!({"path": second.display().to_string()})),
            ("check_for_update", json!({})),
        ],
    );
    for reply in [&replies[0], &replies[3]] {
        assert!(error(reply).contains("without its updater"), "{reply}");
    }
}

#[test]
fn the_update_tools_are_listed_without_a_project_parameter() {
    let (_dir, file, _) = place();
    let input: String = [
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect();
    let mut output = Vec::new();
    bios_mcp::serve(BiosTools::for_agent_in_recents(Recents::at(&file)), std::io::Cursor::new(input), &mut output).unwrap();
    let listed: Value = serde_json::from_str(String::from_utf8(output).unwrap().lines().nth(1).unwrap()).unwrap();
    let tools = listed["result"]["tools"].as_array().unwrap();
    let schema = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"]["properties"].clone();
    assert!(schema("check_for_update").get("project").is_none());
    assert!(schema("update_bios").get("project").is_none());
    assert!(schema("what_is_open").get("project").is_none());
    assert!(schema("read_plasmid").get("project").is_some());
}

/// With a window open on one of several projects, a call that names no project works there; a
/// `project` named still wins, and once the window quits leaving it out is refused again.
#[test]
fn leaving_out_the_project_means_the_one_open_in_the_window() {
    let (_dir, file, plain) = place();
    let other = plain.parent().unwrap().join("Other");
    fs::create_dir(&other).unwrap();
    fs::copy(fixture("wrap_demo.gb"), other.join("only_here.gb")).unwrap();
    let shown = window::file_beside(&file);
    let front = Front { project: Some(ShownProject { path: plain.clone(), name: "Downloads".into() }), place: "edit".into(), ..Front::default() };
    window::publish(&shown, &front).unwrap();
    let replies = calls(
        &file,
        &[
            ("open_project", json!({"path": plain.display().to_string()})),
            ("open_project", json!({"path": other.display().to_string()})),
            ("what_is_open", json!({})),
            ("read_plasmid", json!({"plasmid": "pUC19"})),
            ("read_plasmid", json!({"plasmid": "only_here", "project": "Other"})),
        ],
    );
    let open = body(&replies[2]);
    assert_eq!(open["window_open"], true);
    assert_eq!(open["project"]["path"], plain.display().to_string());
    assert_eq!(body(&replies[3])["name"], "pUC19");
    body(&replies[4]);

    window::closed(&shown).unwrap();
    let replies = calls(&file, &[("what_is_open", json!({})), ("read_plasmid", json!({"plasmid": "pUC19"}))]);
    assert_eq!(body(&replies[0])["window_open"], false);
    assert!(error(&replies[1]).contains("say which project"));
}

#[test]
fn creating_a_project_where_a_folder_already_is_opens_that_folder_as_it_is() {
    let (_dir, file, plain) = place();
    let parent = plain.parent().unwrap().display().to_string();
    let name = plain.file_name().unwrap().to_string_lossy().into_owned();
    let replies = calls(&file, &[("create_project", json!({"name": name, "path": parent})), ("list_plasmids", json!({"project": name}))]);
    let made = body(&replies[0]);
    assert_eq!(made["opened_existing"], true);
    assert_eq!(made["path"], plain.canonicalize().unwrap().display().to_string());
    assert!(!plain.join("bios.json").exists(), "opening writes nothing");
    assert!(body(&replies[1]).to_string().contains("wrap_demo"));
}
