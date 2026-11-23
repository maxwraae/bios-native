//! Bios's own updates through the commands, with a pretend updater: no network, nothing installed.

use std::sync::{Arc, Mutex};

use bios_engine::update::{Check, Installed, Updater};
use bios_engine::{Effect, Session};
use bios_project::{Actor, Project, Who};
use serde_json::json;

/// Answers with a fixed check and counts the installations asked for.
struct Pretend {
    check: Result<Check, String>,
    installs: Mutex<u32>,
}

impl Pretend {
    fn new(check: Result<Check, String>) -> Arc<Self> {
        Arc::new(Pretend { check, installs: Mutex::new(0) })
    }

    fn installs(&self) -> u32 {
        *self.installs.lock().unwrap()
    }
}

impl Updater for Pretend {
    fn check(&self) -> Result<Check, String> {
        self.check.clone()
    }

    fn install(&self) -> Result<Installed, String> {
        *self.installs.lock().unwrap() += 1;
        Ok(Installed {
            from: "0.1.0".into(),
            to: "0.2.0".into(),
            location: "/Applications/Bios.app".into(),
            relaunch: "Bios closes and opens again".into(),
        })
    }
}

fn session(dir: &tempfile::TempDir, updater: Arc<Pretend>) -> Session {
    let project = Project::create(&dir.path().join("p"), "P").unwrap();
    Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None }).with_updater(updater)
}

fn newer() -> Check {
    Check {
        current: "0.1.0".into(),
        latest: Some("0.2.0".into()),
        newer: true,
        notes: "Faster rows.".into(),
        page: "https://github.com/maxwraae/bios-native/releases/tag/v0.2.0".into(),
        cannot_install: None,
    }
}

#[test]
fn a_check_says_what_is_running_what_is_out_and_its_notes() {
    let dir = tempfile::tempdir().unwrap();
    let pretend = Pretend::new(Ok(newer()));
    let out = session(&dir, pretend.clone()).execute("check_for_update", &json!({})).unwrap();
    assert_eq!(out["current_version"], "0.1.0");
    assert_eq!(out["latest_version"], "0.2.0");
    assert_eq!(out["update_available"], true);
    assert_eq!(out["release_notes"], "Faster rows.");
    assert_eq!(out["can_install_itself"], true);
    assert_eq!(pretend.installs(), 0, "a check installs nothing");
}

#[test]
fn no_release_yet_is_an_answer_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let none = Check { current: "0.1.0".into(), page: "https://github.com/maxwraae/bios-native/releases".into(), ..Check::default() };
    let out = session(&dir, Pretend::new(Ok(none))).execute("check_for_update", &json!({})).unwrap();
    assert_eq!(out["latest_version"], json!(null));
    assert_eq!(out["update_available"], false);
    assert!(out["said"].as_str().unwrap().contains("No release"));
}

#[test]
fn a_network_failure_comes_back_as_the_commands_failure() {
    let dir = tempfile::tempdir().unwrap();
    let failure = session(&dir, Pretend::new(Err("couldn't reach GitHub (offline)".into()))).execute("check_for_update", &json!({})).unwrap_err();
    assert!(failure.0.contains("couldn't reach GitHub"), "{failure}");
}

#[test]
fn update_installs_a_newer_release_and_tells_the_agent_to_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let pretend = Pretend::new(Ok(newer()));
    let out = session(&dir, pretend.clone()).execute("update_bios", &json!({})).unwrap();
    assert_eq!(out["updated"], true);
    assert_eq!(out["from_version"], "0.1.0");
    assert_eq!(out["to_version"], "0.2.0");
    assert!(out["next"].as_str().unwrap().contains("reconnect"));
    assert_eq!(pretend.installs(), 1);
}

#[test]
fn update_does_nothing_when_this_is_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let current = Check { newer: false, latest: Some("0.1.0".into()), ..newer() };
    let pretend = Pretend::new(Ok(current));
    let out = session(&dir, pretend.clone()).execute("update_bios", &json!({})).unwrap();
    assert_eq!(out["updated"], false);
    assert_eq!(pretend.installs(), 0);
}

#[test]
fn update_refuses_a_copy_that_cannot_install_itself_and_says_where_to_download() {
    let dir = tempfile::tempdir().unwrap();
    let dev = Check { cannot_install: Some("it runs from a build folder".into()), ..newer() };
    let pretend = Pretend::new(Ok(dev));
    let failure = session(&dir, pretend.clone()).execute("update_bios", &json!({})).unwrap_err();
    assert!(failure.0.contains("build folder") && failure.0.contains("releases/tag/v0.2.0"), "{failure}");
    assert_eq!(pretend.installs(), 0);
}

#[test]
fn without_an_updater_the_commands_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("p"), "P").unwrap();
    let plain = Session::new(project, Who { actor: Actor::Human, source: "test".into(), session: None });
    assert!(plain.execute("check_for_update", &json!({})).unwrap_err().0.contains("without its updater"));
}

#[test]
fn the_update_commands_need_no_project_and_only_the_install_counts_as_a_change() {
    let check = bios_engine::command("check_for_update").unwrap();
    let update = bios_engine::command("update_bios").unwrap();
    assert!(!check.needs_project() && !update.needs_project());
    assert!(bios_engine::command("read_plasmid").unwrap().needs_project());
    assert_eq!((check.effect, update.effect), (Effect::Read, Effect::Write));
}
