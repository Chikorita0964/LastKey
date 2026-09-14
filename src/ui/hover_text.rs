//! Clipped labels reveal their end on pointer hover without changing layout.
use super::theme;
use iced::{
    Color, Element, Event, Font, Length, Point, Rectangle, Size, Theme,
    advanced::{
        Layout, Renderer as _, Shell, Widget, layout, mouse, renderer,
        text::{self, Renderer as _, paragraph::Plain},
        widget::{Tree, tree},
    },
    window,
};
use std::time::{Duration, Instant};

pub fn label<'a, Message: 'a>(
    content: impl Into<String>,
    size: f32,
    font: Font,
    color: Option<Color>,
    centered: bool,
) -> Element<'a, Message> {
    Element::new(Label {
        content: content.into(),
        size,
        font,
        color,
        centered,
    })
}
struct Label {
    content: String,
    size: f32,
    font: Font,
    color: Option<Color>,
    centered: bool,
}
type Paragraph = Plain<<iced::Renderer as text::Renderer>::Paragraph>;
#[derive(Default)]
struct State {
    full: Paragraph,
    clipped: Paragraph,
    motion: Motion,
}
#[derive(Default)]
struct Motion {
    offset: f32,
    transition: Option<(Instant, f32, f32, Duration)>,
    hovered: bool,
}
impl Motion {
    fn advance(&mut self, now: Instant) {
        if let Some((start, from, to, duration)) = self.transition {
            let progress = (now.saturating_duration_since(start).as_secs_f32()
                / duration.as_secs_f32())
            .min(1.0);
            self.offset = from + (to - from) * progress;
            if progress >= 1.0 {
                self.transition = None;
            }
        }
    }
    fn retarget(&mut self, hovered: bool, distance: f32, now: Instant) {
        self.advance(now);
        if hovered != self.hovered {
            self.hovered = hovered;
            let target = if hovered { distance } else { 0.0 };
            let seconds = ((target - self.offset).abs() / 70.0).clamp(0.260, 2.400);
            self.transition = Some((now, self.offset, target, Duration::from_secs_f32(seconds)));
        }
    }
}
impl<Message> Widget<Message, Theme, iced::Renderer> for Label {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Fixed(self.size * 1.3))
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let height = self.size * 1.3;
        let state = tree.state.downcast_mut::<State>();
        let format = text::Text {
            content: self.content.as_str(),
            bounds: Size::new(f32::INFINITY, height),
            size: self.size.into(),
            line_height: text::LineHeight::Relative(1.3),
            font: self.font,
            align_x: text::Alignment::Left,
            align_y: iced::alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: renderer.hint_factor(),
        };
        let changed = state.full.update(format);
        // The box is the text's own width, never the width offered. `Fill`
        // would be resolved against the incoming `max` -- a row hands every
        // child the same loose limits -- so a label beside a trailing icon
        // would swallow the line and push that icon to the far edge. The
        // incoming `max` still clamps the result, and the clip below still
        // truncates, so a label wider than its parent behaves as before.
        let natural = state.full.min_width();
        let node = layout::sized(limits, Length::Shrink, height, |_| {
            Size::new(natural, height)
        });
        let changed = state.clipped.update(text::Text {
            bounds: node.bounds().size(),
            ellipsis: text::Ellipsis::End,
            ..format
        }) || changed;
        if changed {
            state.motion = Motion::default();
        }
        node
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let distance = (state.full.min_width() - bounds.width).max(0.0);
        // A deactivated window keeps no pointer, so the reveal snaps back and
        // asks for no further frames.
        if distance <= 1.0
            || !viewport.intersects(&bounds)
            || matches!(event, Event::Window(window::Event::Unfocused))
        {
            state.motion = Motion::default();
            return;
        }
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        let hovered = cursor.is_over(bounds) && cursor.is_over(*viewport);
        if hovered != state.motion.hovered {
            shell.request_redraw();
        }
        state.motion.retarget(hovered, distance, now);
        if state.motion.transition.is_some() {
            shell.request_redraw_at(now + Duration::from_millis(16));
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let revealing = state.motion.hovered || state.motion.transition.is_some();
        let paragraph = if revealing {
            &state.full
        } else {
            &state.clipped
        };
        let center = if self.centered {
            ((bounds.width - state.full.min_width()) / 2.0).max(0.0)
        } else {
            0.0
        };
        renderer.with_layer(clip, |renderer| {
            renderer.fill_paragraph(
                paragraph.raw(),
                Point::new(bounds.x + center - state.motion.offset, bounds.y),
                self.color.unwrap_or(style.text_color),
                clip,
            )
        });
    }
}

pub fn body<'a, Message: 'a>(content: impl Into<String>) -> Element<'a, Message> {
    label(
        content,
        12.0,
        theme::UI_FONT,
        Some(theme::MUTED_TEXT),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_reversal_continues_from_the_displayed_position() {
        let start = Instant::now();
        let mut motion = Motion::default();
        motion.retarget(true, 70.0, start);
        motion.retarget(false, 70.0, start + Duration::from_millis(500));
        assert!((motion.offset - 35.0).abs() < 0.01);
        motion.advance(start + Duration::from_secs(1));
        assert_eq!(motion.offset, 0.0);
        assert!(motion.transition.is_none());
    }
}
