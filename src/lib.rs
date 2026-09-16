pub mod app;
pub mod core;
pub mod platform;
pub mod protocol;
pub mod settings;

#[cfg(all(windows, feature = "egui-ui"))]
pub mod ui;
