//! Small native drawing widgets. No canvas, image decoder, or polling task is needed.

use iced::{
    Border, Color, Element, Event, Length, Rectangle, Size, Theme,
    advanced::{
        Layout, Renderer as _, Shell, Widget, layout, mouse, renderer,
        widget::tree::{self, Tree},
    },
    touch, window,
};

pub fn logo<'a, Message: 'a>(rgba: &'static [u8], pixels: u32) -> Element<'a, Message> {
    Element::new(Logo { rgba, pixels })
}

struct Logo {
    rgba: &'static [u8],
    pixels: u32,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Logo {
    fn size(&self) -> Size<Length> {
        Size::new(32.0.into(), 32.0.into())
    }
    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, 32.0, 32.0)
    }
    fn draw(
        &self,
        _: &Tree,
        renderer: &mut iced::Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let scale = bounds.width / self.pixels as f32;
        for (index, pixel) in self.rgba.as_chunks::<4>().0.iter().enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + (index as u32 % self.pixels) as f32 * scale,
                        y: bounds.y + (index as u32 / self.pixels) as f32 * scale,
                        width: scale,
                        height: scale,
                    },
                    ..renderer::Quad::default()
                },
                Color::from_rgba8(pixel[0], pixel[1], pixel[2], pixel[3] as f32 / 255.0),
            );
        }
    }
}

pub fn range_slider<'a, Message: 'a>(
    minimum: f32,
    maximum: f32,
    floor: f32,
    enabled: bool,
    accent: Color,
    on_change: impl Fn(bool, f32) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(RangeSlider {
        minimum,
        maximum,
        floor,
        enabled,
        accent,
        on_change: Box::new(on_change),
    })
}

struct RangeSlider<'a, Message> {
    minimum: f32,
    maximum: f32,
    floor: f32,
    enabled: bool,
    accent: Color,
    on_change: Box<dyn Fn(bool, f32) -> Message + 'a>,
}

/// Drag state for the two-handle rail. The reference jumps the grabbed
/// handle to the pointer on press (unless grabbed within 12 px, where the
/// grab offset is preserved), then drags the moving handle while the other
/// stays anchored. Handles may cross: the pair is re-sorted on every move,
/// so a merged pair can be pulled apart in either direction.
#[derive(Default)]
struct RangeState {
    dragging: Option<RangeDrag>,
}

#[derive(Clone, Copy)]
struct RangeDrag {
    anchor: f32,
    offset: f32,
}

/// Pixel distance within which a press keeps its grab offset instead of
/// jumping the handle to the pointer (reference behavior).
const GRAB_RADIUS_PX: f32 = 12.0;

impl<Message> Widget<Message, Theme, iced::Renderer> for RangeSlider<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<RangeState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(RangeState::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, 28.0.into())
    }
    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, 28.0)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<RangeState>();
        if !self.enabled {
            state.dragging = None;
            return;
        }
        let bounds = layout.bounds();
        let track = (bounds.width - 16.0).max(1.0);
        let to_value = |x: f32| ((x - bounds.x - 8.0) / track * 20.0).clamp(0.0, 20.0);
        let to_rounded = |x: f32| {
            (((x - bounds.x - 8.0) / track * 200.0).round() / 10.0).clamp(self.floor, 20.0)
        };
        let position = match event {
            Event::Touch(touch::Event::FingerPressed { position, .. })
            | Event::Touch(touch::Event::FingerMoved { position, .. }) => Some(*position),
            _ => cursor.position(),
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(point) = position.filter(|point| bounds.contains(*point)) {
                    let value = to_value(point.x);
                    let minimum = self.minimum.min(20.0);
                    let maximum = self.maximum.min(20.0);
                    let select_minimum = if minimum == maximum {
                        value <= minimum
                    } else {
                        (value - minimum).abs() <= (value - maximum).abs()
                    };
                    let (selected, anchor) = if select_minimum {
                        (minimum, maximum)
                    } else {
                        (maximum, minimum)
                    };
                    // Near-thumb grabs drag by offset; far presses jump.
                    let offset = if (value - selected).abs() * track / 20.0 <= GRAB_RADIUS_PX {
                        value - selected
                    } else {
                        0.0
                    };
                    state.dragging = Some(RangeDrag { anchor, offset });
                    // Pressing far from either thumb jumps it immediately,
                    // like the reference pointer-down handler.
                    let jumped = to_rounded(point.x - offset * track / 20.0);
                    let (minimum, maximum) = if jumped <= anchor {
                        (jumped, anchor)
                    } else {
                        (anchor, jumped)
                    };
                    shell.publish((self.on_change)(true, minimum));
                    shell.publish((self.on_change)(false, maximum));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
            | Event::Window(window::Event::Unfocused) => {
                if state.dragging.take().is_some() {
                    shell.capture_event();
                }
                return;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {}
            _ => return,
        }
        if let Some(drag) = state.dragging
            && let Some(point) = position
        {
            let moved = to_rounded(point.x - drag.offset * track / 20.0);
            let (minimum, maximum) = if moved <= drag.anchor {
                (moved, drag.anchor)
            } else {
                (drag.anchor, moved)
            };
            shell.publish((self.on_change)(true, minimum));
            shell.publish((self.on_change)(false, maximum));
            shell.capture_event();
        }
    }
    fn draw(
        &self,
        _: &Tree,
        renderer: &mut iced::Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let x = |value: f32| {
            bounds.x + 8.0 + (bounds.width - 16.0).max(0.0) * value.clamp(0.0, 20.0) / 20.0
        };
        let center = bounds.center_y();
        let muted = Color::from_rgb8(203, 213, 225);
        let accent = if self.enabled { self.accent } else { muted };
        // Reference rail is `h-3` with a bordered track; thumbs are `w-4`
        // white circles with a 3 px accent ring.
        for (start, width, color) in [
            (
                bounds.x + 8.0,
                (bounds.width - 16.0).max(0.0),
                Color::from_rgb8(241, 245, 249),
            ),
            (
                x(self.minimum),
                (x(self.maximum) - x(self.minimum)).max(0.0),
                accent,
            ),
        ] {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: start,
                        y: center - 6.0,
                        width,
                        height: 12.0,
                    },
                    border: Border {
                        radius: 6.0.into(),
                        ..Border::default()
                    },
                    ..renderer::Quad::default()
                },
                color,
            );
        }
        for value in [self.minimum, self.maximum] {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: x(value) - 8.0,
                        y: center - 8.0,
                        width: 16.0,
                        height: 16.0,
                    },
                    border: Border {
                        width: 3.0,
                        radius: 8.0.into(),
                        color: accent,
                    },
                    ..renderer::Quad::default()
                },
                Color::WHITE,
            );
        }
    }
    fn mouse_interaction(
        &self,
        _: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Rectangle,
        _: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.enabled && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
