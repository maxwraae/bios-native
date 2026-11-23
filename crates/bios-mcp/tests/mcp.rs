//! An agent's view: JSON-RPC lines in, JSON-RPC lines out.

use std::path::Path;

use bios_mcp::BiosTools;
use bios_project::Project;
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// Sends `calls` as one agent connection and gives back each call's result.
fn connect(root: &Path, calls: &[(&str, Value)]) -> Vec<Value> {
    let mut lines = vec![
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    for (i, (name, arguments)) in calls.iter().enumerate() {
        lines.push(json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));
    }
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut output = Vec::new();
    bios_mcp::serve(BiosTools::for_agent(Project::open(root).unwrap()), std::io::Cursor::new(input), &mut output).unwrap();
    let replies: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    replies.into_iter().skip(1).map(|r| r["result"].clone()).collect()
}

fn body(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    Project::create(dir.path(), "Lab").unwrap();
    dir
}

#[test]
fn every_command_is_listed_as_a_tool_with_its_parameters() {
    let dir = project();
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    );
    let mut output = Vec::new();
    bios_mcp::serve(BiosTools::for_agent(Project::open(dir.path()).unwrap()), std::io::Cursor::new(input), &mut output).unwrap();
    let replies: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();

    assert!(replies[0]["result"]["instructions"].as_str().unwrap().contains("This project is \"Lab\""));
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let commands: Vec<&str> = bios_engine::registry().map(|c| c.name).collect();
    assert_eq!(names, commands);

    let insert = tools.iter().find(|t| t["name"] == "insert_sequence").unwrap();
    assert_eq!(insert["inputSchema"]["required"], json!(["plasmid", "version", "position", "sequence"]));
    assert_eq!(insert["inputSchema"]["properties"]["position"]["type"], "integer");
    assert_eq!(insert["annotations"]["readOnlyHint"], false);
    let read = tools.iter().find(|t| t["name"] == "read_plasmid").unwrap();
    assert_eq!(read["annotations"]["readOnlyHint"], true);
    assert_eq!(read["title"], "Read plasmid");
}

#[test]
fn an_agent_edits_a_plasmid_and_its_work_is_on_record() {
    let dir = project();
    let imported = connect(dir.path(), &[("import_plasmid", json!({ "file": fixture("pUC19.gb") }))]);
    let version = body(&imported[0])["version"].clone();

    let results = connect(
        dir.path(),
        &[
            ("read_plasmid", json!({ "plasmid": "pUC19" })),
            ("insert_sequence", json!({ "plasmid": "pUC19", "version": version, "position": 100, "sequence": "GAATTC" })),
            ("insert_sequence", json!({ "plasmid": "pUC19", "version": version, "position": 100, "sequence": "GAATTC" })),
            ("plasmid_history", json!({ "plasmid": "pUC19" })),
        ],
    );

    // The summary is JSON; the GenBank text follows as its own block.
    assert_eq!(body(&results[0])["length"], 2686);
    assert!(results[0]["content"][1]["text"].as_str().unwrap().starts_with("LOCUS       pUC19"));
    assert!(body(&results[0]).get("genbank").is_none());

    assert_eq!(body(&results[1])["saved"], true);
    // The same edit against the version it has already replaced is refused, as an error result.
    assert_eq!(results[2]["isError"], true);
    assert!(results[2]["content"][0]["text"].as_str().unwrap().starts_with("CONFLICT: nothing was saved"));

    let history = body(&results[3])["history"].as_array().unwrap().clone();
    let last = history.last().unwrap();
    assert_eq!(last["by"], "agent");
    assert_eq!(last["from"], "mcp");
    assert_eq!(last["command"], "insert_sequence");
    assert!(last["session"].is_string());
}

#[test]
fn a_bad_call_is_refused_before_it_reaches_the_engine() {
    let dir = project();
    let results = connect(dir.path(), &[("read_plasmid", json!({ "plasmd": "x" })), ("no_such_tool", json!({}))]);
    assert!(results.iter().all(|r| r.is_null() || r["isError"] == true), "{results:?}");
}
