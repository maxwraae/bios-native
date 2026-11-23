//! Editing a construct by hand: where the caret and the selection are, what a key press does to
//! them and to the bases, and the steps that can be undone. Nothing here draws or saves. The
//! window keeps one of these per open construct and saves through the commands, like an agent.
//!
//! Ported from the Bios web app's editor (`apps/web/src/editor`).

pub mod edit;
pub mod fragment;
pub mod guard;
pub mod range_edit;
pub mod state;
