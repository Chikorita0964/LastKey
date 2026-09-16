//! The egui settings window: page composition, state, cards, and the IPC
//! client. The Iced implementation it replaced was retired in T11; the
//! behavioural contract now lives in `docs/architecture/ui.md`.
//!
//! Doc comments in this tree cite the retired Iced sources with an `iced-ui/`
//! path prefix (for example `iced-ui/widgets.rs:234`). That tree no longer
//! exists in the working copy; recover a cited file from git history (last
//! present before T11).

mod app;
pub mod header;
pub mod inspection;
pub mod ipc_client;
pub mod keycap;
pub mod language;
pub mod mapping;
pub mod message;
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
