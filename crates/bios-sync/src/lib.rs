//! A project that also lives on a Bios server, so that people can share it.
//!
//! The project on this machine stays a whole project: a folder of GenBank files with its full
//! history, open to the window, the command line and any agent, with or without a network. This
//! crate keeps it in step with the same project on a server: it brings in what other people
//! saved there and sends what was saved here, and it never merges. When a plasmid changed in
//! both places, both versions are kept and the person decides.
//!
//! - [`gateway`]: the few things sync asks of a server, as a trait.
//! - [`http`]: that trait over HTTP, to a real server.
//! - [`fake`]: that trait in memory, with the server's rules, for tests here and in the apps.
//! - [`sync`]: the binding kept in the project folder, and the rules.
//! - [`snapshot`]: a construct as it is sent.
//! - [`report`]: what a sync did, in sentences for a person.
//!
//! Nothing here knows of any one server: no address, no key. The caller brings both.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod fake;
pub mod gateway;
pub mod http;
pub mod report;
pub mod snapshot;
pub mod sync;

pub use fake::FakeGateway;
pub use gateway::{Created, Gateway, GatewayError, ProjectListing, RemoteConstruct, RemoteDocument, RemoteProject, Role, SaveOutcome};
pub use http::HttpGateway;
pub use report::{Conflict, Item, Report, Skipped};
pub use snapshot::{Snapshot, SnapshotFeature, SnapshotLocation};
pub use sync::{Connection, Keep, SyncError, connect, connection, publish, resolve, send_again, sync};
