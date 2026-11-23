//! Read mapping in a build of Bios that has no read mapper. The commands are still there, as every
//! capability is a command, and say plainly that read mapping is not in this build.

use std::sync::Arc;

use serde_json::Value;

use crate::verify::{NO_MAPPER, ReadMapper, Unavailable};
use crate::{Args, Failure, Session, fail};

/// The mapper a session of this build starts with: one that maps nothing and says why.
pub fn own_mapper() -> Arc<dyn ReadMapper> {
    Arc::new(Unavailable)
}

pub(crate) fn map_reads(_: &Session, _: &Args) -> Result<Value, Failure> {
    fail(NO_MAPPER)
}
