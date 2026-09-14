//! Illustrative examples only. This module never reads or predicts monitor output.
use iced::{
    Element, Event, Length, Rectangle, Size, Theme,
    advanced::{
        Layout, Shell, Widget, layout, mouse, renderer,
        widget::{Tree, tree},
    },
    window,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Previous,
    Next,
    Toggle,
    Tick,
}

#[derive(Default)]
pub struct Preview {
    pub example: usize,
    pub phase: usize,
    pub playing: bool,
}

impl Preview {
    pub fn update(&mut self, action: Action) {
        match action {
            Action::Previous => {
                self.example = (self.example + 2) % 3;
                self.phase = 0;
            }
            Action::Next => {
                self.example = (self.example + 1) % 3;
                self.phase = 0;
            }
            Action::Toggle => self.playing = !self.playing,
            Action::Tick if self.playing => self.phase = (self.phase + 1) % 4,
            Action::Tick => {}
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

/// The clock is mounted with the example, and only requests frames while
/// visible and while the window is awake.
pub fn clock<'a, Message: Clone + 'a>(
    preview: &Preview,
    awake: bool,
    tick: Message,
) -> Element<'a, Message> {
    Element::new(Clock {
        playing: preview.playing && awake,
        example: preview.example,
        tick,
    })
}
struct Clock<Message> {
    playing: bool,
    example: usize,
    tick: Message,
}
#[derive(Default)]
struct State {
    next: Option<Instant>,
    example: usize,
}
impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for Clock<Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(1.0))
    }
    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, 1.0)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if !self.playing || !viewport.intersects(&layout.bounds()) || state.example != self.example
        {
            state.next = None;
            state.example = self.example;
        }
        if self.playing
            && viewport.intersects(&layout.bounds())
            && let Event::Window(window::Event::RedrawRequested(now)) = event
        {
            if state.next.is_some_and(|next| *now >= next) {
                shell.publish(self.tick.clone());
                state.next = None;
            }
            let next = *state.next.get_or_insert(*now + Duration::from_millis(850));
            shell.request_redraw_at(next);
        }
    }
    fn draw(
        &self,
        _: &Tree,
        _: &mut iced::Renderer,
        _: &Theme,
        _: &renderer::Style,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn badges_keep_integers_and_fractional_ranges_readable() {
        assert_eq!(delay_label(0, 0), "0 ms");
        assert_eq!(delay_label(2000, 2000), "2 ms");
        assert_eq!(delay_label(2100, 4000), "2.1~4 ms");
    }
    #[test]
    fn examples_show_the_resolution_then_the_new_key_without_running_the_filter() {
        let mut preview = Preview::default();
        preview.update(Action::Tick);
        assert_eq!(preview.phase, 0);
        preview.update(Action::Toggle);
        for expected in [(false, true), (false, false), (true, true)] {
            assert_eq!(preview.held(), (true, false));
            preview.update(Action::Tick);
            assert_eq!(preview.held(), expected);
            preview.update(Action::Tick);
            assert_eq!(preview.held(), (false, true));
            preview.update(Action::Next);
        }
        assert_eq!(preview.example, 0);
    }
}
