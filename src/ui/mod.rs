//! The egui settings window: page composition, state, cards, and the IPC
//! client. The behavioural contract lives in `docs/architecture/ui.md` and the
//! visual contract in `docs/architecture/design.md`.
//!
//! Every colour, radius, shadow, and spacing this tree paints comes from
//! [`theme`], which is the single owner of the visual contract. A view module
//! that finds itself reaching for a literal should add a named token there
//! instead, so the design doc has exactly one place to describe.

mod app;
pub mod header;
pub mod inspection;
pub mod ipc_client;
pub mod keycap;
pub mod language;
pub mod mapping;
pub mod message;
pub mod motion;
pub mod preview;
pub mod profiles;
pub mod state;
#[cfg(test)]
mod state_tests;
pub mod theme;
pub mod timeline;
pub mod timing;

pub use app::run;

mod icon {
    include!(concat!(env!("OUT_DIR"), "/lastkey_icon.rs"));
}
