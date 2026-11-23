//! The HTTP gateway against a tiny server on 127.0.0.1 that answers from a script: what is asked
//! (method, path, headers, body) and what each kind of answer is taken to mean.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::Duration;

use bios_core::genbank;
use bios_sync::{Gateway, GatewayError, HttpGateway, Role, SaveOutcome, Snapshot};
use serde_json::{Value, json};

/// One request as the server received it.
#[derive(Debug)]
struct Seen {
    method: String,
    path: String,
    /// Names in lower case.
    headers: BTreeMap<String, String>,
    body: String,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

enum Canned {
    Json(u16, Value),
    /// A status with a body that is not the server's JSON.
    Text(u16, &'static str),
    /// Bytes written as they are, in place of an HTTP answer.
    Raw(&'static [u8]),
    /// The request is read and never answered.
    Silence(Duration),
}

struct Server {
    base: String,
    thread: JoinHandle<Vec<Seen>>,
}

impl Server {
    /// Answers one request with each of `script`, in order, then stops listening.
    fn start(script: Vec<Canned>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for canned in script {
                let (mut stream, _) = listener.accept().unwrap();
                seen.push(read_request(&mut stream));
                answer(&mut stream, canned);
            }
            seen
        });
        Server { base, thread }
    }

    fn gateway(&self) -> HttpGateway {
        HttpGateway::new(&self.base, Some("the-key".into()), Some("the-token".into()))
    }

    /// Everything that was asked, once the script has run out.
    fn seen(self) -> Vec<Seen> {
        self.thread.join().unwrap()
    }
}

fn read_request(stream: &mut TcpStream) -> Seen {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "the request ended before its headers did");
        bytes.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8(bytes[..head_end].to_vec()).unwrap();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap().split(' ');
    let (method, path) = (first.next().unwrap().to_string(), first.next().unwrap().to_string());
    let headers: BTreeMap<String, String> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    let length: usize = headers.get("content-length").map_or(0, |l| l.parse().unwrap());
    while bytes.len() < head_end + length {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "the request ended before its body did");
        bytes.extend_from_slice(&chunk[..n]);
    }
    Seen { method, path, headers, body: String::from_utf8(bytes[head_end..head_end + length].to_vec()).unwrap() }
}

fn answer(stream: &mut TcpStream, canned: Canned) {
    let write = |stream: &mut TcpStream, status: u16, kind: &str, body: &str| {
        let head = format!("HTTP/1.1 {status} X\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len());
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body.as_bytes()).unwrap();
    };
    match canned {
        Canned::Json(status, body) => write(stream, status, "application/json", &body.to_string()),
        Canned::Text(status, body) => write(stream, status, "text/html", body),
        Canned::Raw(bytes) => stream.write_all(bytes).unwrap(),
        Canned::Silence(long) => std::thread::sleep(long),
    }
}

const ID: &str = "0b54c1d2-7a3e-4f6b-9c0d-1e2f3a4b5c6d";
const FILE: &str = "\
LOCUS       pGFP                      20 bp    DNA     circular SYN 18-NOV-2026
FEATURES             Location/Qualifiers
     CDS             3..10
                     /label=\"GFP\"
                     /gene=\"gfp\"
ORIGIN
        1 acgtacgtag acgtacgtag
//
";

fn snapshot() -> Snapshot {
    Snapshot::of(&genbank::read(FILE).unwrap()).unwrap()
}

fn features() -> Value {
    json!([{ "name": "GFP", "type": "CDS", "start": 2, "end": 9, "strand": 1, "notes": { "gene": ["gfp"] } }])
}

/// A document as the server sends one.
fn document(version: u64) -> Value {
    json!({
        "constructId": ID,
        "version": version,
        "etag": format!("{version}:abc"),
        "textSha": "abc",
        "contentSha": "def",
        "text": FILE,
        "sequence": "ACGTACGTAGACGTACGTAG",
        "circular": true,
        "features": features(),
        "updatedAt": "2026-11-18T14:03:00.000Z",
        "name": "pGFP",
    })
}

#[test]
fn every_request_carries_the_credentials_it_was_given_and_no_others() {
    let server = Server::start(vec![Canned::Json(200, json!({ "projects": [] })), Canned::Json(200, json!({ "projects": [] }))]);
    server.gateway().list_projects().unwrap();
    HttpGateway::new(&format!("{}/", server.base), Some("only-a-key".into()), None).list_projects().unwrap();

    let seen = server.seen();
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("GET", "/conversations"));
    assert_eq!(seen[0].headers["x-api-key"], "the-key");
    assert_eq!(seen[0].headers["x-bios-user"], "the-token");
    assert_eq!(seen[1].path, "/conversations", "a trailing slash on the address is not doubled");
    assert_eq!(seen[1].headers["x-api-key"], "only-a-key");
    assert!(!seen[1].headers.contains_key("x-bios-user"));
    // And they are not something a log line can pick up by printing the gateway.
    let printed = format!("{:?}", HttpGateway::new("http://127.0.0.1:1", Some("the-key".into()), Some("the-token".into())));
    assert!(!printed.contains("the-key") && !printed.contains("the-token"), "{printed}");
}

#[test]
fn projects_are_listed_with_what_this_account_may_do_and_one_can_be_made() {
    let listed = json!({ "projects": [
        { "id": "p1", "title": "Mine", "role": "owner", "constructCount": 3, "lead": null },
        { "id": "p2", "title": "Shared", "role": "editor" },
        { "id": "p3", "title": "Read only", "role": "viewer" },
        { "id": "p4", "title": "A role from the future", "role": "admin" },
        { "title": "No id" },
        "not a project",
    ] });
    let server = Server::start(vec![Canned::Json(200, listed), Canned::Json(200, json!({ "conversation": { "id": "p9", "title": "New chat" } }))]);
    let gateway = server.gateway();

    let projects = gateway.list_projects().unwrap();
    let made = gateway.create_project("Golden Gate parts").unwrap();

    let roles: Vec<(&str, Role)> = projects.iter().map(|p| (p.id.as_str(), p.role)).collect();
    assert_eq!(roles, vec![("p1", Role::Owner), ("p2", Role::Editor), ("p3", Role::Viewer), ("p4", Role::Viewer)], "an unknown role may only look");
    assert_eq!((made.id.as_str(), made.title.as_str(), made.role), ("p9", "Golden Gate parts", Role::Owner));
    let seen = server.seen();
    assert_eq!((seen[1].method.as_str(), seen[1].path.as_str()), ("POST", "/projects/owned/conversations"));
    assert_eq!(seen[1].headers["content-type"], "application/json");
    assert_eq!(seen[1].json(), json!({ "title": "Golden Gate parts" }));
}

#[test]
fn a_project_is_listed_without_its_bases_and_with_its_folders() {
    let listing = json!({
        "constructs": [
            { "id": ID, "name": "pGFP", "path": format!("backbones/2026/{ID}.gb"), "projectKey": "owned", "circular": true, "role": "focus", "length": 20 },
            { "id": "c2", "name": "pTop", "path": "pTop.gb" },
            { "name": "no id" },
        ],
        "folders": ["backbones", "backbones/2026", 7],
        "folderMeta": {},
    });
    let server = Server::start(vec![Canned::Json(200, listing)]);

    let listing = server.gateway().list_constructs("p 1/..").unwrap();

    let placed: Vec<(&str, &str, &str)> = listing.constructs.iter().map(|c| (c.id.as_str(), c.name.as_str(), c.folder.as_str())).collect();
    assert_eq!(placed, vec![(ID, "pGFP", "backbones/2026"), ("c2", "pTop", "")]);
    assert_eq!(listing.constructs[0].path, format!("backbones/2026/{ID}.gb"));
    assert_eq!(listing.folders, vec!["backbones", "backbones/2026"]);
    let seen = server.seen();
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/conversations/p%201%2F../constructs?lean=1", "an id cannot change which route is asked");
}

#[test]
fn a_document_is_read_and_only_the_servers_own_not_found_means_gone() {
    let server = Server::start(vec![
        Canned::Json(200, json!({ "document": document(4) })),
        Canned::Json(404, json!({ "error": "not found" })),
        Canned::Text(404, "<html>404 page not found</html>"),
        Canned::Json(200, json!({ "document": { "constructId": "someone-elses", "version": 1, "etag": "1:x", "text": FILE } })),
    ]);
    let gateway = server.gateway();

    let read = gateway.read_document(ID).unwrap();
    assert_eq!((read.construct_id.as_str(), read.version, read.etag.as_str()), (ID, 4, "4:abc"));
    assert_eq!((read.text.as_str(), read.sequence.as_str(), read.circular), (FILE, "ACGTACGTAGACGTACGTAG", true));
    assert_eq!((read.name.as_deref(), read.updated_at.as_deref()), (Some("pGFP"), Some("2026-11-18T14:03:00.000Z")));

    let gone = gateway.read_document(ID).unwrap_err();
    assert_eq!(gone, GatewayError::NotFound("this plasmid".into()));
    assert_eq!(gone.to_string(), "this plasmid is not on the server, or this account may not open it");
    // A page from something in between says nothing about the plasmid.
    assert_eq!(gateway.read_document(ID), Err(GatewayError::Unexpected("status 404".into())));
    assert!(matches!(gateway.read_document(ID), Err(GatewayError::Unexpected(_))), "an answer about another plasmid is not this one");

    let seen = server.seen();
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("GET", format!("/documents/{ID}").as_str()));
}

#[test]
fn a_save_names_the_version_it_was_made_from_and_every_answer_is_told_apart() {
    let server = Server::start(vec![
        Canned::Json(200, json!({ "document": document(5) })),
        Canned::Json(412, json!({ "error": "conflict", "conflict": true, "document": document(7) })),
        Canned::Json(412, json!({ "error": "conflict", "conflict": true, "document": null })),
        Canned::Json(422, json!({ "error": "a document with no bases is a failed read, not an edit" })),
        Canned::Json(404, json!({ "error": "not found" })),
        Canned::Json(401, json!({ "error": "unauthorized" })),
        Canned::Text(500, "Internal Server Error"),
    ]);
    let gateway = server.gateway();
    let save = || gateway.save_document(ID, "4:abc", &snapshot(), "request-1");

    match save().unwrap() {
        SaveOutcome::Saved(saved) => assert_eq!((saved.version, saved.etag.as_str()), (5, "5:abc")),
        SaveOutcome::Conflict(_) => panic!("a 200 is a save"),
    }
    match save().unwrap() {
        SaveOutcome::Conflict(Some(theirs)) => assert_eq!((theirs.version, theirs.updated_at.as_deref()), (7, Some("2026-11-18T14:03:00.000Z"))),
        other => panic!("{other:?}"),
    }
    assert_eq!(save().unwrap(), SaveOutcome::Conflict(None));
    let refused = save().unwrap_err();
    assert_eq!(refused, GatewayError::Refused { status: 422, reason: "a document with no bases is a failed read, not an edit".into() });
    assert_eq!(refused.to_string(), "the server refused the request: a document with no bases is a failed read, not an edit");
    assert_eq!(save(), Err(GatewayError::NotFound("this plasmid".into())));
    assert_eq!(save(), Err(GatewayError::NotSignedIn));
    assert_eq!(save(), Err(GatewayError::Unexpected("status 500".into())));

    let seen = server.seen();
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("PUT", format!("/documents/{ID}").as_str()));
    // The token exactly as the server gave it: that is what it compares against.
    assert_eq!(seen[0].headers["if-match"], "4:abc");
    assert_eq!(seen[0].headers["x-api-key"], "the-key");
    assert_eq!(seen[0].headers["x-bios-user"], "the-token");
    assert_eq!(seen[0].headers["content-type"], "application/json");
    assert_eq!(
        seen[0].json(),
        json!({
            "text": snapshot().text,
            "sequence": "ACGTACGTAGACGTACGTAG",
            "circular": true,
            "features": features(),
            "name": "pGFP",
            "requestId": "request-1",
        })
    );
}

/// The construct as the routes that write a project's own copy take it.
fn project_copy(project_key: &str, path: &str) -> Value {
    json!({
        "projectKey": project_key,
        "path": path,
        "name": "pGFP",
        "circular": true,
        "length": 20,
        "sequence": "ACGTACGTAGACGTACGTAG",
        "features": features(),
        "sha": "",
    })
}

#[test]
fn a_new_plasmid_is_created_then_filed_in_the_project_then_moved_to_its_folder() {
    let file = format!("{ID}.gb");
    let server = Server::start(vec![
        Canned::Json(201, json!({ "construct": { "id": ID }, "document": document(1) })),
        Canned::Json(201, json!({ "projectKey": "p1", "constructs": [{ "id": ID, "path": file, "name": "pGFP", "length": 20 }] })),
        Canned::Json(200, json!({ "ok": true, "path": format!("backbones/2026/{file}") })),
        // And one for the top of the project: no move is asked for.
        Canned::Json(201, json!({ "construct": { "id": ID }, "document": document(1) })),
        Canned::Json(201, json!({ "projectKey": "p1", "constructs": [{ "id": ID, "path": file, "name": "pGFP", "length": 20 }] })),
    ]);
    let gateway = server.gateway();

    let created = gateway.create_document("p1", "backbones/2026", &snapshot(), "request-2").unwrap();
    let at_top = gateway.create_document("p1", "", &snapshot(), "request-3").unwrap();

    assert_eq!((created.document.construct_id.as_str(), created.document.etag.as_str()), (ID, "1:abc"));
    assert_eq!(created.path, format!("backbones/2026/{file}"));
    assert_eq!(at_top.path, file);
    let seen = server.seen();
    assert_eq!(seen.len(), 5);
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", "/documents"));
    assert_eq!(seen[0].json()["requestId"], "request-2");
    assert_eq!(seen[0].json()["text"], snapshot().text);
    assert_eq!((seen[1].method.as_str(), seen[1].path.as_str()), ("POST", "/conversations/p1/constructs"));
    // Filed under its id, which is how the server knows it is the construct just made.
    assert_eq!(seen[1].json(), json!({ "constructs": [project_copy("pending", &file)] }));
    assert_eq!((seen[2].method.as_str(), seen[2].path.as_str()), ("PUT", format!("/conversations/p1/constructs/{ID}/path").as_str()));
    assert_eq!(seen[2].json(), json!({ "folder": "backbones/2026" }));
}

#[test]
fn a_move_a_rename_and_the_refresh_of_the_projects_own_copy() {
    let server = Server::start(vec![
        Canned::Json(200, json!({ "ok": true, "path": format!("{ID}.gb") })),
        Canned::Json(200, json!({ "ok": true, "name": "pGFP_v2" })),
        Canned::Json(200, json!({ "ok": true, "count": 1, "results": [{ "path": "x.gb", "ok": true, "sha": "s" }] })),
        Canned::Json(200, json!({ "ok": false, "count": 0, "results": [{ "path": "x.gb", "ok": false, "conflict": true, "sha": "s" }] })),
        Canned::Json(400, json!({ "error": "name required" })),
    ]);
    let gateway = server.gateway();
    let path = format!("backbones/{ID}.gb");

    assert_eq!(gateway.move_construct("p1", ID, "").unwrap(), format!("{ID}.gb"));
    assert_eq!(gateway.rename_construct("p1", ID, "pGFP v2").unwrap(), "pGFP_v2", "the name comes back as the server kept it");
    gateway.sync_project("p1", &[(path.clone(), snapshot())]).unwrap();
    assert!(matches!(gateway.sync_project("p1", &[(path.clone(), snapshot())]), Err(GatewayError::Unexpected(_))));
    assert_eq!(gateway.rename_construct("p1", ID, " "), Err(GatewayError::Refused { status: 400, reason: "name required".into() }));

    let seen = server.seen();
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("PUT", format!("/conversations/p1/constructs/{ID}/path").as_str()));
    assert_eq!(seen[0].json(), json!({ "folder": null }), "the top of the project is null, not an empty name");
    assert_eq!((seen[1].method.as_str(), seen[1].path.as_str()), ("PUT", format!("/conversations/p1/constructs/{ID}").as_str()));
    assert_eq!(seen[1].json(), json!({ "name": "pGFP v2" }));
    assert_eq!((seen[2].method.as_str(), seen[2].path.as_str()), ("POST", "/conversations/p1/sync"));
    // What the web app sends: the constructs, and no claim about what was there before.
    assert_eq!(seen[2].json(), json!({ "snapshots": [project_copy("p1", &path)] }));
}

#[test]
fn an_answer_that_is_not_a_bios_servers_is_an_error_and_never_a_panic() {
    let server = Server::start(vec![
        Canned::Text(200, "<html>Welcome to the captive portal</html>"),
        Canned::Json(200, json!(["an", "array"])),
        Canned::Json(200, json!({ "document": { "constructId": ID, "version": "four", "etag": 4, "text": null } })),
        Canned::Json(200, json!({ "document": { "constructId": ID, "version": -1, "etag": "", "text": FILE } })),
        Canned::Json(200, json!({ "projects": "many" })),
        Canned::Json(200, json!({ "constructs": { "id": ID } })),
        Canned::Json(201, json!({ "conversation": {} })),
        Canned::Json(200, json!({ "ok": true })),
        Canned::Json(200, json!({ "document": null })),
        Canned::Text(302, ""),
        Canned::Raw(b"\x00\xff\x13 this is not HTTP at all\r\n\r\n"),
        Canned::Raw(b""),
        Canned::Raw(b"HTTP/1.1 200 OK\r\ncontent-length: 500\r\n\r\n{\"document\":"),
    ]);
    let gateway = server.gateway();

    assert_eq!(gateway.read_document(ID), Err(GatewayError::Unexpected("status 200 without a readable answer".into())));
    assert!(matches!(gateway.read_document(ID), Err(GatewayError::Unexpected(_))));
    assert_eq!(gateway.read_document(ID), Err(GatewayError::Unexpected("the plasmid was missing from the answer".into())));
    assert!(matches!(gateway.read_document(ID), Err(GatewayError::Unexpected(_))), "a version below zero and an empty etag are not a document");
    assert!(matches!(gateway.list_projects(), Err(GatewayError::Unexpected(_))));
    assert!(matches!(gateway.list_constructs("p1"), Err(GatewayError::Unexpected(_))));
    assert!(matches!(gateway.create_project("x"), Err(GatewayError::Unexpected(_))));
    assert!(matches!(gateway.move_construct("p1", ID, "a"), Err(GatewayError::Unexpected(_))));
    assert!(matches!(gateway.save_document(ID, "4:abc", &snapshot(), "r"), Err(GatewayError::Unexpected(_))));
    // A redirect is not followed: the credentials are for this server.
    assert_eq!(gateway.read_document(ID), Err(GatewayError::Unexpected("status 302".into())));
    for _ in 0..3 {
        let failed = gateway.read_document(ID).unwrap_err();
        assert!(matches!(failed, GatewayError::Unreachable(_) | GatewayError::Unexpected(_)), "{failed:?}");
    }
    assert_eq!(server.seen().len(), 13);
}

/// The whole path once: a real project folder, the rules of sync, and HTTP underneath.
#[test]
fn publishing_and_then_sending_an_edit_over_http_asks_for_exactly_this() {
    use bios_project::{Actor, Change, Project, Who};
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let who = Who { actor: Actor::Human, source: "test".into(), session: None };
    let change = |summary: &str| Change { command: "test".into(), params: Value::Null, summary: summary.into() };
    let made = project.create_construct(genbank::read(FILE).unwrap(), "", None, &who, change("Made")).unwrap();
    let mut edited = made.construct.clone();
    edited.sequence.push_str("TTTT");
    // What the edit will be sent as, so the scripted server can answer with the etag a real one would.
    let mut as_saved = edited.clone();
    as_saved.comments = project.read(&made.id).unwrap().construct.comments;
    let text = bios_core::genbank_write::write(&as_saved).unwrap();
    let sha: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let mut saved = document(2);
    saved["etag"] = json!(format!("2:{sha}"));

    let file = format!("{ID}.gb");
    let row = json!({ "id": ID, "name": "pGFP", "path": file });
    let listed = || Canned::Json(200, json!({ "constructs": [row.clone()], "folders": [] }));
    let server = Server::start(vec![
        // publish
        Canned::Json(200, json!({ "conversation": { "id": "p9", "title": "Lab" } })),
        Canned::Json(200, json!({ "constructs": [], "folders": [] })),
        Canned::Json(201, json!({ "construct": { "id": ID }, "document": document(1) })),
        Canned::Json(201, json!({ "projectKey": "p9", "constructs": [{ "id": ID, "path": file, "name": "pGFP", "length": 20 }] })),
        listed(),
        // a sync with nothing to do
        Canned::Json(200, json!({ "projects": [{ "id": "p9", "title": "Lab", "role": "owner" }] })),
        listed(),
        Canned::Json(200, json!({ "document": document(1) })),
        // a sync after an edit here
        Canned::Json(200, json!({ "projects": [{ "id": "p9", "title": "Lab", "role": "owner" }] })),
        listed(),
        Canned::Json(200, json!({ "document": document(1) })),
        Canned::Json(200, json!({ "document": saved })),
        Canned::Json(200, json!({ "ok": true, "count": 1, "results": [] })),
        listed(),
    ]);
    let gateway = server.gateway();

    let report = bios_sync::publish(&project, &gateway, "the test server", "Lab", &who).unwrap();
    assert!(report.is_clean() && report.added_there.len() == 1, "{report}");
    assert!(bios_sync::sync(&project, &gateway, &who).unwrap().is_empty());
    project.save(&made.id, &made.sha, &edited, &who, change("Added four bases")).unwrap();
    let report = bios_sync::sync(&project, &gateway, &who).unwrap();
    assert!(report.is_clean() && report.sent.len() == 1, "{report}");

    let seen = server.seen();
    let asked: Vec<String> = seen.iter().map(|s| format!("{} {}", s.method, s.path)).collect();
    assert_eq!(
        asked,
        vec![
            "POST /projects/owned/conversations".to_string(),
            "GET /conversations/p9/constructs?lean=1".into(),
            "POST /documents".into(),
            "POST /conversations/p9/constructs".into(),
            "GET /conversations/p9/constructs?lean=1".into(),
            "GET /conversations".into(),
            "GET /conversations/p9/constructs?lean=1".into(),
            format!("GET /documents/{ID}"),
            "GET /conversations".into(),
            "GET /conversations/p9/constructs?lean=1".into(),
            format!("GET /documents/{ID}"),
            format!("PUT /documents/{ID}"),
            "POST /conversations/p9/sync".into(),
            "GET /conversations/p9/constructs?lean=1".into(),
        ]
    );
    assert_eq!(seen[11].headers["if-match"], "1:abc");
    assert_eq!(seen[11].json()["text"], text);
    assert_eq!(seen[11].json()["sequence"], "ACGTACGTAGACGTACGTAGTTTT");
    assert_eq!(seen[12].json()["snapshots"][0]["path"], file);
    // The folder remembers where it is shared and nothing that could open the server.
    let kept = std::fs::read_to_string(project.root().join(".bios/remote.json")).unwrap();
    assert!(kept.contains("\"server\": \"the test server\"") && kept.contains("\"project\": \"p9\""), "{kept}");
    assert!(!kept.contains("the-key") && !kept.contains("the-token") && !kept.contains("127.0.0.1"), "{kept}");
}

#[test]
fn a_server_that_does_not_answer_is_given_up_on() {
    let server = Server::start(vec![Canned::Silence(Duration::from_millis(1500))]);
    let gateway = server.gateway().with_timeout(Duration::from_millis(200));
    let started = std::time::Instant::now();

    let failed = gateway.list_projects().unwrap_err();

    assert!(matches!(failed, GatewayError::Unreachable(_)), "{failed:?}");
    assert!(started.elapsed() < Duration::from_millis(1200), "it waited {:?}", started.elapsed());
    assert!(failed.to_string().starts_with("the server could not be reached ("), "{failed}");
    assert!(failed.to_string().ends_with("check the connection and try again"), "{failed}");
    server.seen();
}

#[test]
fn nothing_listening_is_unreachable() {
    // Bound and dropped, so the port is one nothing answers on.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let gateway = HttpGateway::new(&format!("http://127.0.0.1:{port}"), None, None);
    assert!(matches!(gateway.list_projects(), Err(GatewayError::Unreachable(_))));
    assert!(matches!(
        HttpGateway::new("not an address", None, None).list_projects(),
        Err(GatewayError::Unreachable(_) | GatewayError::Unexpected(_))
    ));
}
