//! The gateway over HTTP, to a real Bios server.
//!
//! Every answer is treated as something that may be wrong: its size is bounded, its JSON is read
//! field by field, and a "not found" counts only when it is the server's own. That last one
//! matters most, because "not found" is the one answer sync acts on by letting go of a plasmid:
//! a page from a proxy or a mistyped address must never be read as it.

use std::time::Duration;

use serde_json::{Value, json};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::gateway::{Created, Gateway, GatewayError, ProjectListing, RemoteConstruct, RemoteDocument, RemoteProject, Role, SaveOutcome, folder_of};
use crate::snapshot::Snapshot;

/// No answer of a Bios server that sync reads comes near this. More is not read.
const MAX_ANSWER: u64 = 32 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct HttpGateway {
    agent: ureq::Agent,
    base: String,
    api_key: Option<String>,
    user_token: Option<String>,
}

// Written by hand so that the credentials can never reach a log through `{:?}`.
impl std::fmt::Debug for HttpGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpGateway").field("base", &self.base).finish_non_exhaustive()
    }
}

enum Method {
    Get,
    Post,
    Put,
}

struct Answer {
    status: u16,
    body: Vec<u8>,
}

impl HttpGateway {
    /// A gateway to the server at `base_url`, such as `https://bios.example`. The credentials
    /// are sent on every request as `x-api-key` and `x-bios-user`; where they come from is the
    /// caller's business, and they are never written anywhere.
    pub fn new(base_url: &str, api_key: Option<String>, user_token: Option<String>) -> HttpGateway {
        HttpGateway { agent: agent(TIMEOUT), base: base_url.trim().trim_end_matches('/').to_string(), api_key, user_token }
    }

    /// The same gateway, giving up on a request after `timeout`.
    pub fn with_timeout(mut self, timeout: Duration) -> HttpGateway {
        self.agent = agent(timeout);
        self
    }

    fn dress<B>(&self, mut request: ureq::RequestBuilder<B>, if_match: Option<&str>) -> ureq::RequestBuilder<B> {
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }
        if let Some(token) = &self.user_token {
            request = request.header("x-bios-user", token);
        }
        if let Some(etag) = if_match {
            request = request.header("if-match", etag);
        }
        request
    }

    fn request(&self, method: Method, path: &str, if_match: Option<&str>, body: Option<&Value>) -> Result<Answer, GatewayError> {
        let url = format!("{}{path}", self.base);
        let text = body.map(Value::to_string).unwrap_or_else(|| "{}".to_string());
        let sent = match method {
            Method::Get => self.dress(self.agent.get(&url), if_match).call(),
            Method::Post => self.dress(self.agent.post(&url), if_match).content_type("application/json").send(text.as_str()),
            Method::Put => self.dress(self.agent.put(&url), if_match).content_type("application/json").send(text.as_str()),
        };
        let mut response = sent.map_err(not_sent)?;
        let status = response.status().as_u16();
        let body = response.body_mut().with_config().limit(MAX_ANSWER).read_to_vec().map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => GatewayError::Unexpected(format!("an answer of more than {} MB", MAX_ANSWER / 1024 / 1024)),
            other => not_sent(other),
        })?;
        Ok(Answer { status, body })
    }

    /// The JSON of a successful answer, or the error the status stands for. `what` names the
    /// thing asked about, for the sentence a "not found" becomes.
    fn json(&self, method: Method, path: &str, body: Option<&Value>, what: &str) -> Result<Value, GatewayError> {
        let answer = self.request(method, path, None, body)?;
        read(&answer, what)
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    // The operating system's TLS and its own root certificates.
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        // A status is an answer to be read, not an error to be thrown.
        .http_status_as_error(false)
        // The credentials are for this server. A redirect is not followed with them.
        .max_redirects(0)
        .tls_config(tls)
        .build()
        .into()
}

/// A request that got no answer. Nothing in the message comes from the credentials.
fn not_sent(e: ureq::Error) -> GatewayError {
    match e {
        ureq::Error::Timeout(_) => GatewayError::Unreachable("it took too long to answer".into()),
        ureq::Error::HostNotFound => GatewayError::Unreachable("no server was found at that address".into()),
        ureq::Error::ConnectionFailed | ureq::Error::Io(_) => GatewayError::Unreachable("the connection failed".into()),
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => {
            GatewayError::Unexpected("the request could not be written; check the server's address".into())
        }
        other => GatewayError::Unreachable(other.to_string()),
    }
}

fn read(answer: &Answer, what: &str) -> Result<Value, GatewayError> {
    let parsed: Option<Value> = serde_json::from_slice(&answer.body).ok();
    let reason = || parsed.as_ref().and_then(|v| v.get("error")).and_then(Value::as_str).unwrap_or_default().to_string();
    match answer.status {
        200 | 201 => {
            parsed.filter(Value::is_object).ok_or_else(|| GatewayError::Unexpected(format!("status {} without a readable answer", answer.status)))
        }
        // The server's own "not found" is JSON with its reason. Anything else with this status
        // came from something in between, and says nothing about whether the thing exists.
        404 if !reason().is_empty() => Err(GatewayError::NotFound(what.to_string())),
        401 => Err(GatewayError::NotSignedIn),
        status @ 400..=499 if parsed.is_some() => Err(GatewayError::Refused { status, reason: reason() }),
        status => Err(GatewayError::Unexpected(format!("status {status}"))),
    }
}

/// One part of a path, with everything that could change the path's meaning escaped.
fn part(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn garbled(what: &str) -> GatewayError {
    GatewayError::Unexpected(format!("{what} was missing from the answer"))
}

/// A document out of an answer. One without an id, a version, an etag and its text is not one.
fn document(value: Option<&Value>) -> Option<RemoteDocument> {
    let d = value?;
    let etag = text(d, "etag").filter(|e| !e.is_empty())?;
    Some(RemoteDocument {
        construct_id: text(d, "constructId")?,
        version: d.get("version").and_then(Value::as_u64)?,
        etag,
        text: text(d, "text")?,
        sequence: text(d, "sequence").unwrap_or_default(),
        circular: d.get("circular").and_then(Value::as_bool).unwrap_or(false),
        name: text(d, "name"),
        updated_at: text(d, "updatedAt"),
    })
}

/// The construct as the routes that write a project's own copy take it.
fn project_copy(project_key: &str, path: &str, snapshot: &Snapshot) -> Value {
    json!({
        "projectKey": project_key,
        "path": path,
        "name": snapshot.name,
        "circular": snapshot.circular,
        "length": snapshot.sequence.len(),
        "sequence": snapshot.sequence,
        "features": snapshot.features,
        // The server works this out itself.
        "sha": "",
    })
}

fn document_body(snapshot: &Snapshot, request_id: &str) -> Value {
    json!({
        "text": snapshot.text,
        "sequence": snapshot.sequence,
        "circular": snapshot.circular,
        "features": snapshot.features,
        "name": snapshot.name,
        "requestId": request_id,
    })
}

impl Gateway for HttpGateway {
    fn list_projects(&self) -> Result<Vec<RemoteProject>, GatewayError> {
        let answer = self.json(Method::Get, "/conversations", None, "the list of projects")?;
        let projects = answer.get("projects").and_then(Value::as_array).ok_or_else(|| garbled("the list of projects"))?;
        Ok(projects
            .iter()
            .filter_map(|p| {
                let role = match p.get("role").and_then(Value::as_str) {
                    Some("owner") => Role::Owner,
                    Some("editor") => Role::Editor,
                    // Anything else may look and not touch.
                    _ => Role::Viewer,
                };
                Some(RemoteProject { id: text(p, "id")?, title: text(p, "title").unwrap_or_default(), role })
            })
            .collect())
    }

    fn create_project(&self, title: &str) -> Result<RemoteProject, GatewayError> {
        let answer = self.json(Method::Post, "/projects/owned/conversations", Some(&json!({ "title": title })), "the new project")?;
        let id = answer.get("conversation").and_then(|c| text(c, "id")).filter(|id| !id.is_empty()).ok_or_else(|| garbled("the new project's id"))?;
        Ok(RemoteProject { id, title: title.to_string(), role: Role::Owner })
    }

    fn list_constructs(&self, project: &str) -> Result<ProjectListing, GatewayError> {
        let answer = self.json(Method::Get, &format!("/conversations/{}/constructs?lean=1", part(project)), None, "this project")?;
        let rows = answer.get("constructs").and_then(Value::as_array).ok_or_else(|| garbled("the project's plasmids"))?;
        let constructs = rows
            .iter()
            .filter_map(|row| {
                let path = text(row, "path").unwrap_or_default();
                Some(RemoteConstruct { id: text(row, "id")?, name: text(row, "name").unwrap_or_default(), folder: folder_of(&path), path })
            })
            .collect();
        let folders = answer
            .get("folders")
            .and_then(Value::as_array)
            .map(|f| f.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        Ok(ProjectListing { constructs, folders })
    }

    fn read_document(&self, construct: &str) -> Result<RemoteDocument, GatewayError> {
        let answer = self.json(Method::Get, &format!("/documents/{}", part(construct)), None, "this plasmid")?;
        let found = document(answer.get("document")).ok_or_else(|| garbled("the plasmid"))?;
        if found.construct_id != construct {
            return Err(GatewayError::Unexpected("the answer was about a different plasmid".into()));
        }
        Ok(found)
    }

    fn create_document(&self, project: &str, folder: &str, snapshot: &Snapshot, request_id: &str) -> Result<Created, GatewayError> {
        let answer = self.json(Method::Post, "/documents", Some(&document_body(snapshot, request_id)), "the new plasmid")?;
        let mut made = document(answer.get("document")).ok_or_else(|| garbled("the new plasmid"))?;
        // Filed in the project under `<id>.gb`, which is how the server knows it is the construct
        // that was just made and not a second one.
        let file = format!("{}.gb", made.construct_id);
        let attach = json!({ "constructs": [project_copy("pending", &file, snapshot)] });
        let filed = self.json(Method::Post, &format!("/conversations/{}/constructs", part(project)), Some(&attach), "this project")?;
        let row = filed
            .get("constructs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .ok_or_else(|| garbled("the plasmid's place in the project"))?;
        let id = text(row, "id").ok_or_else(|| garbled("the plasmid's id in the project"))?;
        let mut path = text(row, "path").unwrap_or(file);
        // A server that filed it as a construct of its own: that one is the project's, so that
        // is the one to keep in step.
        if id != made.construct_id {
            made = self.read_document(&id)?;
        }
        if !folder.is_empty() {
            path = self.move_construct(project, &id, folder)?;
        }
        Ok(Created { document: made, path })
    }

    fn save_document(&self, construct: &str, etag: &str, snapshot: &Snapshot, request_id: &str) -> Result<SaveOutcome, GatewayError> {
        let body = document_body(snapshot, request_id);
        let answer = self.request(Method::Put, &format!("/documents/{}", part(construct)), Some(etag), Some(&body))?;
        if answer.status == 412 {
            // The current copy comes with the refusal when the server sent one that can be read.
            let current = serde_json::from_slice::<Value>(&answer.body).ok();
            return Ok(SaveOutcome::Conflict(document(current.as_ref().and_then(|v| v.get("document")))));
        }
        let saved = read(&answer, "this plasmid")?;
        document(saved.get("document")).map(SaveOutcome::Saved).ok_or_else(|| garbled("the saved plasmid"))
    }

    fn move_construct(&self, project: &str, construct: &str, folder: &str) -> Result<String, GatewayError> {
        let to = if folder.is_empty() { Value::Null } else { Value::String(folder.to_string()) };
        let path = format!("/conversations/{}/constructs/{}/path", part(project), part(construct));
        let answer = self.json(Method::Put, &path, Some(&json!({ "folder": to })), "this plasmid")?;
        text(&answer, "path").ok_or_else(|| garbled("the plasmid's new place"))
    }

    fn rename_construct(&self, project: &str, construct: &str, name: &str) -> Result<String, GatewayError> {
        let path = format!("/conversations/{}/constructs/{}", part(project), part(construct));
        let answer = self.json(Method::Put, &path, Some(&json!({ "name": name })), "this plasmid")?;
        text(&answer, "name").ok_or_else(|| garbled("the plasmid's new name"))
    }

    fn sync_project(&self, project: &str, constructs: &[(String, Snapshot)]) -> Result<(), GatewayError> {
        let snapshots: Vec<Value> = constructs.iter().map(|(path, snapshot)| project_copy(project, path, snapshot)).collect();
        let answer =
            self.json(Method::Post, &format!("/conversations/{}/sync", part(project)), Some(&json!({ "snapshots": snapshots })), "this project")?;
        if answer.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(GatewayError::Unexpected("not every plasmid in the project's list was refreshed".into()))
        }
    }
}
