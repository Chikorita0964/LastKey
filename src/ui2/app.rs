use std::sync::Arc;

use eframe::{App, CreationContext, Frame, NativeOptions};
use egui::{IconData, Ui, ViewportBuilder};

const WINDOW_TITLE: &str = "LastKey Settings";
const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 800.0;

pub fn run() -> eframe::Result {
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))),
    )
}

fn window_icon() -> Arc<IconData> {
    Arc::new(IconData {
        rgba: super::icon::WINDOW_ICON_RGBA.to_vec(),
        width: super::icon::WINDOW_ICON_WIDTH,
        height: super::icon::WINDOW_ICON_HEIGHT,
    })
}

struct SettingsApp {}

impl SettingsApp {
    fn new(_cc: &CreationContext<'_>) -> Self {
        Self {}
    }
}

impl App for SettingsApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.heading(WINDOW_TITLE);
        });
    }
}
