//! The egui settings window. `src/ui/` holds the Iced implementation this
//! replaces; it is no longer compiled and is kept only as the behavioural
//! specification until cutover.

mod app;

pub use app::run;

mod icon {
    include!(concat!(env!("OUT_DIR"), "/lastkey_icon.rs"));
}
