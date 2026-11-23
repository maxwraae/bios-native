//! What sync needs from a Bios server, and nothing else.
//!
//! [`crate::http::HttpGateway`] speaks to a real server and [`crate::fake::FakeGateway`] keeps
//! the same rules in memory, so every rule of sync is tested without a network.

use crate::snapshot::Snapshot;

/// What an account may do in a project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Editor,
    /// May read everything and change nothing.
    Viewer,
}

impl Role {
    pub fn can_write(self) -> bool {
        !matches!(self, Role::Viewer)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteProject {
    pub id: String,
    pub title: String,
    pub role: Role,
}

/// One construct as a project's listing shows it: no bases, no version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteConstruct {
    pub id: String,
    /// The name the project's listing shows. A server keeps it as one word, so `My plasmid` is
    /// listed as `My_plasmid`.
    pub name: String,
    /// The construct's file inside the project on the server, such as `backbones/<id>.gb`. The
    /// call that refreshes the listing addresses the construct by it.
    pub path: String,
    /// The folder part of `path`, with `/` between its parts. Empty at the top of the project.
    pub folder: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectListing {
    pub constructs: Vec<RemoteConstruct>,
    /// Every folder, empty ones included.
    pub folders: Vec<String>,
}

/// One construct as the server holds it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteDocument {
    pub construct_id: String,
    pub version: u64,
    /// The token a save presents to say which version it was made from. It changes whenever
    /// anything about the document changes.
    pub etag: String,
    /// The GenBank text.
    pub text: String,
    pub sequence: String,
    pub circular: bool,
    /// The name in the server's records. Not every answer carries it.
    pub name: Option<String>,
    /// When this version was saved there, as the server wrote it (RFC 3339).
    pub updated_at: Option<String>,
}

/// A construct that was just created on the server and put in a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub document: RemoteDocument,
    /// Where the project keeps it; see [`RemoteConstruct::path`].
    pub path: String,
}

/// What a save came to. A refusal is not an error: it is the answer sync exists to handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(RemoteDocument),
    /// Nothing was saved: the server's copy is not the version named, or this account may not
    /// change it. The server's current copy comes back when it sent one.
    Conflict(Option<RemoteDocument>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayError {
    /// The server answered that this does not exist, or that this account may not see it. The
    /// only answer that means "gone".
    NotFound(String),
    /// The server did not accept who is asking.
    NotSignedIn,
    /// The server understood and said no, with its reason.
    Refused { status: u16, reason: String },
    /// No answer arrived: no network, no such server, or it took too long.
    Unreachable(String),
    /// An answer arrived that is not what a Bios server sends.
    Unexpected(String),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatewayError::NotFound(what) => write!(f, "{what} is not on the server, or this account may not open it"),
            GatewayError::NotSignedIn => write!(f, "the server did not accept the sign-in; sign in again and retry"),
            GatewayError::Refused { reason, .. } if reason.is_empty() => write!(f, "the server refused the request and gave no reason"),
            GatewayError::Refused { reason, .. } => write!(f, "the server refused the request: {reason}"),
            GatewayError::Unreachable(detail) => write!(f, "the server could not be reached ({detail}); check the connection and try again"),
            GatewayError::Unexpected(detail) => write!(f, "the server gave an answer Bios does not understand ({detail}); nothing was changed by it"),
        }
    }
}

impl std::error::Error for GatewayError {}

/// The operations of a Bios server that sync uses. Every call is made as one account; where its
/// credentials come from is the caller's business.
pub trait Gateway {
    /// Every project this account can open, with what it may do there.
    fn list_projects(&self) -> Result<Vec<RemoteProject>, GatewayError>;

    /// Makes an empty project owned by this account.
    fn create_project(&self, title: &str) -> Result<RemoteProject, GatewayError>;

    fn list_constructs(&self, project: &str) -> Result<ProjectListing, GatewayError>;

    fn read_document(&self, construct: &str) -> Result<RemoteDocument, GatewayError>;

    /// Creates a construct from `snapshot` and puts it in `project` at `folder` (empty for the
    /// top of the project).
    fn create_document(&self, project: &str, folder: &str, snapshot: &Snapshot, request_id: &str) -> Result<Created, GatewayError>;

    /// Saves `snapshot` as the next version of `construct`, if `etag` still names the server's
    /// copy. Sending the same `request_id` again answers as the first time did, so a save whose
    /// answer was lost can be repeated safely.
    fn save_document(&self, construct: &str, etag: &str, snapshot: &Snapshot, request_id: &str) -> Result<SaveOutcome, GatewayError>;

    /// Moves a construct to `folder` (empty for the top of the project) and gives back its new path.
    fn move_construct(&self, project: &str, construct: &str, folder: &str) -> Result<String, GatewayError>;

    /// Renames a construct without touching its bases, and gives back the name as the server kept it.
    fn rename_construct(&self, project: &str, construct: &str, name: &str) -> Result<String, GatewayError>;

    /// Refreshes the project's own copy of each construct, addressed by path, from which its
    /// listing takes names and features. The web app makes this call after saving.
    fn sync_project(&self, project: &str, constructs: &[(String, Snapshot)]) -> Result<(), GatewayError>;
}

/// The folder part of a path on the server.
pub(crate) fn folder_of(path: &str) -> String {
    path.rsplit_once('/').map(|(folder, _)| folder.to_string()).unwrap_or_default()
}
