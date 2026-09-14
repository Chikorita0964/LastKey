//! Illustrative examples only. This module never reads or predicts monitor output.

use super::message::PreviewAction;

#[derive(Default)]
pub struct Preview {
    pub example: usize,
    pub phase: usize,
    pub playing: bool,
}

impl Preview {
    pub fn update(&mut self, action: PreviewAction) {
        match action {
            PreviewAction::Previous => {
                self.example = (self.example + 2) % 3;
                self.phase = 0;
            }
            PreviewAction::Next => {
                self.example = (self.example + 1) % 3;
                self.phase = 0;
            }
            PreviewAction::Toggle => self.playing = !self.playing,
            PreviewAction::Tick if self.playing => self.phase = (self.phase + 1) % 4,
            PreviewAction::Tick => {}
        }
    }

    pub fn held(&self) -> (bool, bool) {
        (
            self.phase == 0 || (self.phase == 1 && self.example == 2),
            self.phase >= 2 || (self.phase == 1 && self.example != 1),
        )
    }
}

pub fn delay_label(min: u32, max: u32) -> String {
    let number = |micros| {
        let value = format!("{:.1}", micros as f32 / 1_000.0);
        value.strip_suffix(".0").unwrap_or(&value).to_owned()
    };
    if min == max {
        format!("{} ms", number(min))
    } else {
        format!("{}~{} ms", number(min), number(max))
    }
}
