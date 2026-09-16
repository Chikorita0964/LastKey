//! The egui settings window. `src/ui/` holds the Iced implementation this
//! replaces; it is no longer compiled and is kept only as the behavioural
//! specification until cutover.

mod app;
pub mod header;
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
