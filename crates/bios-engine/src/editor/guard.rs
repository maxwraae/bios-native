//! The edits a construct refuses, such as one that would leave it empty. Ported from the web app's `editGuard.ts`.
//!
//! An empty editor is never an edit. Emptiness is taken as evidence that a read failed, never as
//! a change worth keeping: empty saves once destroyed a 47 kb construct and a 1 Mb assembly. The
//! save refuses an empty construct too, but by then the empty sequence would already be the one
//! on screen, which reads as "my data is gone". This refuses it one step earlier, where the
//! editor's state becomes the construct.
//!
//! So a construct that was emptied on purpose, by selecting everything and deleting, is refused
//! as well. Delete the file to be rid of it.

use bios_core::Construct;

/// The bases of whatever the editor holds. Empty when it holds nothing.
pub fn bases_of(data: Option<&Construct>) -> &str {
    data.map_or("", |construct| construct.sequence.as_str())
}

/// May this editor state be accepted as the construct?
///
/// False for anything with no bases: an editor that has not been given its sequence yet, a
/// failed read, a parse that was cut short. The caller drops it and keeps what it had.
pub fn accepts_edit(data: Option<&Construct>) -> bool {
    !bases_of(data).is_empty()
}
