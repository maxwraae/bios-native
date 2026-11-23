//! Updates in the window: a quiet check at start (at most once a day), Help ▸ Check for Updates…,
//! and craft-update's prompt offering the new version. The checking and installing are the
//! engine's `check_for_update` and `update_bios`, run off the window's thread, so the window does
//! nothing an agent could not.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use bios_engine::Session;
use bios_project::{Actor, Who};
use craft_update::updater::Status;
use craft_update::{CheckResultDialog, CheckResultResponse, Release, StartupPrompt, StartupPromptResponse};
use serde_json::{Value, json};

/// How long a quiet check at start counts for.
const QUIET_EVERY: Duration = Duration::from_secs(24 * 3600);

/// Where the time of the last quiet check is kept, beside the window's settings.
const STAMP: &str = "update-check.txt";

enum Done {
    Checked(Result<Value, String>),
    Installed(Result<Value, String>),
}

#[derive(Default)]
pub struct Updates {
    /// The quiet check at start may run. Off in tests and pictures, which must not ask the network.
    enabled: bool,
    started: bool,
    /// The check running was asked for from the Help menu, so its outcome is shown either way.
    requested: bool,
    status: Option<Status>,
    prompt: Option<StartupPrompt>,
    work: Option<Receiver<Done>>,
    closing: bool,
}

impl Updates {
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Help ▸ Check for Updates….
    pub fn check_now(&mut self) {
        if self.work.is_none() {
            self.requested = true;
            self.status = Some(Status::Checking);
            self.run("check_for_update", Done::Checked);
        }
    }

    /// Runs `command` on its own thread; its answer arrives at a later frame.
    fn run(&mut self, command: &'static str, done: fn(Result<Value, String>) -> Done) {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name(format!("bios-{command}")).spawn(move || {
            let who = Who { actor: Actor::Human, source: "window".into(), session: None };
            let result = Session::for_app(who).and_then(|s| s.execute(command, &json!({}))).map_err(|f| f.0);
            let _ = tx.send(done(result));
        });
        match spawned {
            Ok(_) => self.work = Some(rx),
            Err(e) => self.finish(done(Err(format!("could not start the check ({e})")))),
        }
    }

    /// One frame: start the quiet check, take a finished answer, draw the prompt or the result.
    pub fn frame(&mut self, ctx: &egui::Context) {
        if self.enabled && !std::mem::replace(&mut self.started, true) && quiet_check_due() {
            self.run("check_for_update", Done::Checked);
        }
        if let Some(done) = self.work.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.work = None;
            self.finish(done);
        }
        if self.work.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        // An update installed in this window, by the prompt or by a command: close, and the
        // helper started at exit opens the new version.
        if bios_update::relaunch_pending() && !std::mem::replace(&mut self.closing, true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.show_prompt(ctx);
        if let Some(status) = &self.status {
            match CheckResultDialog::new(&bios_update::APP, bios_update::VERSION).show(ctx, status) {
                CheckResultResponse::Close => self.status = None,
                CheckResultResponse::CheckAgain => {
                    self.status = None;
                    self.check_now();
                }
                CheckResultResponse::RestartToUpdate | CheckResultResponse::None => {}
            }
        }
    }

    fn show_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.prompt else { return };
        match prompt.show(ctx) {
            StartupPromptResponse::UpdateAndRestart if !prompt.is_installing => {
                prompt.is_installing = true;
                prompt.clear_message();
                self.run("update_bios", Done::Installed);
            }
            StartupPromptResponse::Download => {
                ctx.open_url(egui::OpenUrl::new_tab(prompt.release.html_url.clone()));
                self.prompt = None;
            }
            StartupPromptResponse::Later => self.prompt = None,
            StartupPromptResponse::UpdateAndRestart | StartupPromptResponse::Pending => {}
        }
        if self.prompt.as_ref().is_some_and(|p| !p.open) {
            self.prompt = None;
        }
    }

    fn finish(&mut self, done: Done) {
        let requested = std::mem::take(&mut self.requested);
        match done {
            Done::Checked(Ok(found)) if found["update_available"] == json!(true) => {
                self.status = None;
                let can_install = found["can_install_itself"] == json!(true);
                self.prompt = Some(StartupPrompt::with_self_update(&bios_update::APP, release(&found), bios_update::running_version(), can_install));
            }
            Done::Checked(Ok(_)) => self.status = requested.then_some(Status::UpToDate),
            Done::Checked(Err(why)) if requested => self.status = Some(Status::Failed(why)),
            Done::Checked(Err(why)) => eprintln!("bios: the check for updates failed: {why}"),
            // `relaunch_pending` closes the window at the next frame.
            Done::Installed(Ok(done)) if done["updated"] == json!(true) => {}
            Done::Installed(Ok(done)) => self.installing_stopped(done["said"].as_str().unwrap_or("Bios is already the newest version.").to_string()),
            Done::Installed(Err(why)) => self.installing_stopped(why),
        }
    }

    fn installing_stopped(&mut self, why: String) {
        if let Some(prompt) = &mut self.prompt {
            prompt.is_installing = false;
            prompt.set_message(why);
        }
    }
}

/// The release a check found, as the prompt shows it.
fn release(found: &Value) -> Release {
    let latest = found["latest_version"].as_str().unwrap_or_default();
    Release {
        tag: format!("v{latest}"),
        version: craft_update::parse_version(latest),
        name: format!("Bios {latest}"),
        html_url: found["release_page"].as_str().unwrap_or_default().to_string(),
        notes: found["release_notes"].as_str().unwrap_or_default().to_string(),
        ..Release::default()
    }
}

/// Whether a day has passed since the last quiet check; if so, it is written down as made now.
fn quiet_check_due() -> bool {
    let Some(dir) = bios_project::recents::settings_dir() else { return false };
    let stamp = dir.join(STAMP);
    let now = chrono::Utc::now().timestamp();
    let last = std::fs::read_to_string(&stamp).ok().and_then(|t| t.trim().parse::<i64>().ok());
    if last.is_some_and(|last| (0..QUIET_EVERY.as_secs() as i64).contains(&(now - last))) {
        return false;
    }
    let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&stamp, now.to_string()));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_found_release_becomes_the_prompts_release() {
        let found = json!({
            "latest_version": "0.2.0", "release_page": "https://github.com/maxwraae/bios-native/releases/tag/v0.2.0",
            "release_notes": "Faster rows.", "update_available": true, "can_install_itself": true,
        });
        let r = release(&found);
        assert_eq!((r.tag.as_str(), r.version_label().as_str(), r.notes.as_str()), ("v0.2.0", "0.2.0", "Faster rows."));
    }

    #[test]
    fn a_check_that_finds_a_newer_release_opens_the_prompt_and_a_requested_one_says_up_to_date() {
        let mut updates = Updates::default();
        updates.finish(Done::Checked(Ok(json!({ "update_available": true, "latest_version": "9.0.0", "can_install_itself": false }))));
        assert!(updates.prompt.as_ref().is_some_and(|p| !p.is_self_updating));
        let mut updates = Updates { requested: true, ..Updates::default() };
        updates.finish(Done::Checked(Ok(json!({ "update_available": false }))));
        assert_eq!(updates.status, Some(Status::UpToDate));
        let mut quiet = Updates::default();
        quiet.finish(Done::Checked(Err("offline".into())));
        assert_eq!(quiet.status, None, "a quiet check that fails shows nothing");
    }

    #[test]
    fn a_failed_install_reopens_the_prompt_with_the_reason() {
        let mut updates = Updates::default();
        updates.finish(Done::Checked(Ok(json!({ "update_available": true, "latest_version": "9.0.0", "can_install_itself": true }))));
        if let Some(p) = &mut updates.prompt {
            p.is_installing = true;
        }
        updates.finish(Done::Installed(Err("the disk is full".into())));
        let prompt = updates.prompt.as_ref().unwrap();
        assert!(!prompt.is_installing);
        assert_eq!(prompt.message().as_deref(), Some("the disk is full"));
    }
}
