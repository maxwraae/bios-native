//! Sharing through the commands: two people, two project folders, one pretend server.

use std::path::Path;

use bios_engine::{Server, Session};
use bios_project::{Actor, Project, Who};
use bios_sync::{FakeGateway, Role};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// One person's Bios: a project folder of their own, signed in to the lab's server.
fn person(dir: &tempfile::TempDir, name: &str, gateway: FakeGateway) -> Session {
    let project = Project::create(&dir.path().join(name), "Lab").unwrap();
    let who = Who { actor: Actor::Human, source: "test".into(), session: None };
    Session::new(project, who).with_server(Server { name: "lab server".into(), gateway: Box::new(gateway) })
}

fn bases(s: &Session, plasmid: &str) -> String {
    s.execute("get_sequence", &json!({ "plasmid": plasmid })).unwrap()["sequence"].as_str().unwrap().to_string()
}

fn said(report: &Value) -> Vec<String> {
    report["said"].as_array().unwrap().iter().map(|line| line.as_str().unwrap().to_string()).collect()
}

/// Ana's project with wrap_demo in it, shared, and Ben joined as `role`.
fn shared(role: Role) -> (tempfile::TempDir, Session, Session) {
    let dir = tempfile::tempdir().unwrap();
    let server = FakeGateway::new("ana");
    let ana = person(&dir, "ana", server.as_account("ana"));
    ana.execute("import_plasmid", &json!({ "file": fixture("wrap_demo.gb") })).unwrap();
    let published = ana.execute("share_project", &json!({ "title": "Splicing" })).unwrap();
    assert_eq!(published["shared_as"], "Splicing");
    let remote = ana.execute("sharing_status", &json!({})).unwrap()["project_on_server"].as_str().unwrap().to_string();
    server.share(&remote, "ben", role);
    let ben = person(&dir, "ben", server.as_account("ben"));
    (dir, ana, ben)
}

#[test]
fn a_project_is_shared_joined_and_kept_in_step() {
    let (_dir, ana, ben) = shared(Role::Editor);
    let status = ana.execute("sharing_status", &json!({})).unwrap();
    assert_eq!((&status["shared"], &status["server"], &status["plasmids_in_step"]), (&json!(true), &json!("lab server"), &json!(1)));

    // Ben finds the project on the server and joins it: the plasmid arrives with its features.
    let listed = ben.execute("server_projects", &json!({})).unwrap();
    assert_eq!(listed["projects"], json!([{ "id": status["project_on_server"], "title": "Splicing", "role": "editor" }]));
    let joined = ben.execute("join_shared_project", &json!({ "project": "splicing" })).unwrap();
    assert_eq!((&joined["joined"], &joined["role"]), (&json!("Splicing"), &json!("editor")));
    assert_eq!(joined["report"]["added_here"], json!(["wrap_demo"]));
    assert_eq!(bases(&ben, "wrap_demo"), bases(&ana, "wrap_demo"));

    // Ben edits; after both sync, Ana has his change as a version in her own history.
    let version = ben.execute("read_plasmid", &json!({ "plasmid": "wrap_demo" })).unwrap()["version"].clone();
    ben.execute("insert_sequence", &json!({ "plasmid": "wrap_demo", "position": 0, "sequence": "GGGG", "version": version })).unwrap();
    assert_eq!(ben.execute("sync_project", &json!({})).unwrap()["sent"], json!(["wrap_demo"]));
    let brought = ana.execute("sync_project", &json!({})).unwrap();
    assert_eq!((&brought["brought_in"], &brought["in_step"]), (&json!(["wrap_demo"]), &json!(true)));
    assert!(bases(&ana, "wrap_demo").starts_with("GGGGACGT"));
    let history = ana.execute("plasmid_history", &json!({ "plasmid": "wrap_demo" })).unwrap();
    assert_eq!(history["history"].as_array().unwrap().len(), 2);
    // With nothing new on either side a sync says so and changes nothing.
    assert_eq!(said(&ana.execute("sync_project", &json!({})).unwrap()), ["Everything is in step with lab server."]);
}

#[test]
fn a_plasmid_changed_on_both_sides_is_a_conflict_until_a_person_settles_it() {
    let (_dir, ana, ben) = shared(Role::Editor);
    ben.execute("join_shared_project", &json!({ "project": "Splicing" })).unwrap();
    let edit = |s: &Session, add: &str| {
        let version = s.execute("read_plasmid", &json!({ "plasmid": "wrap_demo" })).unwrap()["version"].clone();
        s.execute("insert_sequence", &json!({ "plasmid": "wrap_demo", "position": 0, "sequence": add, "version": version })).unwrap();
    };
    edit(&ana, "AAAA");
    edit(&ben, "CCCC");
    assert_eq!(ana.execute("sync_project", &json!({})).unwrap()["sent"], json!(["wrap_demo"]));

    let stuck = ben.execute("sync_project", &json!({})).unwrap();
    assert_eq!(stuck["in_step"], false);
    assert_eq!(stuck["conflicts"][0]["plasmid"], "wrap_demo");
    // Neither side was touched.
    assert!(bases(&ben, "wrap_demo").starts_with("CCCC") && bases(&ana, "wrap_demo").starts_with("AAAA"));

    // Ben takes Ana's. His own version stays in his history and can be gone back to.
    let settled = ben.execute("settle_conflict", &json!({ "plasmid": "wrap_demo", "keep": "theirs" })).unwrap();
    assert_eq!(settled["kept"], "theirs");
    assert!(bases(&ben, "wrap_demo").starts_with("AAAA"));
    let history = ben.execute("plasmid_history", &json!({ "plasmid": "wrap_demo" })).unwrap();
    let summaries: Vec<&str> = history["history"].as_array().unwrap().iter().map(|e| e["summary"].as_str().unwrap()).collect();
    assert!(summaries.iter().any(|s| s.starts_with("Inserted 4 bases")), "{summaries:?}");
    assert_eq!(ben.execute("sync_project", &json!({})).unwrap()["in_step"], true);

    let refused = ben.execute("settle_conflict", &json!({ "plasmid": "wrap_demo", "keep": "both" })).unwrap_err();
    assert_eq!(refused.0, "keep is \"mine\" or \"theirs\", not \"both\"");
}

#[test]
fn someone_who_may_only_read_gets_changes_and_sends_none() {
    let (_dir, ana, ben) = shared(Role::Viewer);
    assert_eq!(ben.execute("join_shared_project", &json!({ "project": "Splicing" })).unwrap()["role"], "viewer");
    let version = ben.execute("read_plasmid", &json!({ "plasmid": "wrap_demo" })).unwrap()["version"].clone();
    ben.execute("insert_sequence", &json!({ "plasmid": "wrap_demo", "position": 0, "sequence": "TTTT", "version": version })).unwrap();
    let report = ben.execute("sync_project", &json!({})).unwrap();
    assert_eq!(report["sent"], json!([]));
    assert_eq!(report["in_step"], false);
    assert!(!bases(&ana, "wrap_demo").starts_with("TTTT"));
    assert!(ana.execute("sync_project", &json!({})).unwrap()["brought_in"].as_array().unwrap().is_empty());
}

#[test]
fn without_a_server_the_sharing_commands_say_what_to_set_up() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("alone"), "Alone").unwrap();
    let alone = Session::new(project, Who { actor: Actor::Human, source: "test".into(), session: None });
    let status = alone.execute("sharing_status", &json!({})).unwrap();
    assert_eq!((&status["shared"], &status["server_set_up"]), (&json!(false), &Value::Null));
    for command in ["server_projects", "share_project", "sync_project"] {
        let refused = alone.execute(command, &json!({})).unwrap_err();
        assert!(refused.0.starts_with("No Bios server is set up for this session."), "{command}: {}", refused.0);
    }
}

#[test]
fn the_server_is_set_up_from_its_address_and_goes_by_its_host_name() {
    let set = |pairs: &'static [(&'static str, &'static str)]| move |key: &str| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string());
    assert!(bios_engine::server_from(set(&[])).is_none());
    assert!(bios_engine::server_from(set(&[("BIOS_SERVER", "  "), ("BIOS_SERVER_KEY", "k")])).is_none());
    let server = bios_engine::server_from(set(&[("BIOS_SERVER", "https://bios.example.org/gw/"), ("BIOS_SERVER_TOKEN", "t")])).unwrap();
    assert_eq!(server.name, "bios.example.org");
    assert_eq!(bios_engine::server_from(set(&[("BIOS_SERVER", "http://127.0.0.1:8093")])).unwrap().name, "127.0.0.1");
}
