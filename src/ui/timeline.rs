//! Bounded, opt-in display history of the engine's monitor events. Never persisted.

use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot};
use iced::{
    Color, Element, Event, Length, Rectangle, Size, Theme,
    advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, renderer, widget::Tree},
    window,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const WINDOW_MICROS: u64 = 1_000_000;
const MAX_INTERVALS: usize = 512;

#[derive(Default)]
pub enum MonitorState {
    #[default]
    Stopped,
    Starting,
    Recording(Timeline),
    Stopping(Timeline),
}

impl MonitorState {
    pub fn timeline(&self) -> Option<&Timeline> {
        match self {
            Self::Recording(timeline) | Self::Stopping(timeline) => Some(timeline),
            _ => None,
        }
    }
    pub fn resynchronize(&mut self) {
        if let Self::Recording(timeline) | Self::Stopping(timeline) = self {
            timeline.clear();
        }
    }
}

#[derive(Clone, Copy)]
struct Interval {
    key: usize,
    start: u64,
    end: u64,
}

pub struct Timeline {
    intervals: VecDeque<Interval>,
    held_since: [Option<u64>; 4],
    anchor: Option<(Instant, u64)>,
    pub decision: MonitorDecision,
    pub physical: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            intervals: VecDeque::with_capacity(MAX_INTERVALS),
            held_since: [None; 4],
            anchor: None,
            decision: MonitorDecision::Immediate,
            physical: false,
        }
    }
}

impl Timeline {
    pub fn clear(&mut self) {
        self.intervals.clear();
        self.held_since = [None; 4];
        self.anchor = None;
        self.decision = MonitorDecision::Immediate;
    }
    pub fn accept(&mut self, event: MonitorSnapshot, measuring: bool, received: Instant) {
        let physical = !event.filter_enabled || measuring;
        if self.physical != physical {
            self.clear();
        }
        self.physical = physical;
        // Keep backend deltas intact; arrival time only positions the first event on screen.
        if self.anchor.is_none() {
            self.anchor = Some((received, event.elapsed_micros));
        }
        self.decision = event.decision;
        if physical {
            if let Some(edge) = event.physical {
                self.edge(edge, event.elapsed_micros);
            }
        } else {
            for edge in event.outputs {
                self.edge(edge, event.elapsed_micros);
            }
        }
        let cutoff = event.elapsed_micros.saturating_sub(WINDOW_MICROS);
        while self
            .intervals
            .front()
            .is_some_and(|interval| interval.end < cutoff)
        {
            self.intervals.pop_front();
        }
    }
    fn edge(&mut self, edge: MonitorEdge, at: u64) {
        let key = index(edge.key);
        if edge.pressed {
            self.held_since[key].get_or_insert(at);
        } else if let Some(start) = self.held_since[key].take() {
            if self.intervals.len() == MAX_INTERVALS {
                self.intervals.pop_front();
            }
            self.intervals.push_back(Interval {
                key,
                start,
                end: at,
            });
        }
    }
    fn now(&self) -> u64 {
        self.anchor.map_or(0, |(arrival, offset)| {
            offset.saturating_add(arrival.elapsed().as_micros() as u64)
        })
    }
    pub fn held(&self, key: KeySlot) -> bool {
        self.held_since[index(key)].is_some()
    }
}

const fn index(key: KeySlot) -> usize {
    match key {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}

pub fn graph<'a, Message: 'a>(timeline: Option<&'a Timeline>) -> Element<'a, Message> {
    Element::new(Graph(timeline))
}

struct Graph<'a>(Option<&'a Timeline>);

impl<Message> Widget<Message, Theme, iced::Renderer> for Graph<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, 144.0.into())
    }
    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, 144.0)
    }
    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && let Some(timeline) = self.0
        {
            let animate = timeline.held_since.iter().any(Option::is_some)
                || timeline.intervals.back().is_some_and(|interval| {
                    timeline.now().saturating_sub(interval.end) < WINDOW_MICROS
                });
            if animate {
                shell.request_redraw_at(*now + Duration::from_millis(16));
            }
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
        for row in 0..4 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: bounds.y + row as f32 * 36.0,
                        width: bounds.width,
                        height: 30.0,
                    },
                    ..renderer::Quad::default()
                },
                Color::from_rgb8(241, 245, 249),
            );
        }
        for tick in 0..=10 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + bounds.width * tick as f32 / 10.0,
                        y: bounds.y,
                        width: 1.0,
                        height: bounds.height,
                    },
                    ..renderer::Quad::default()
                },
                Color::from_rgb8(226, 232, 240),
            );
        }
        let Some(timeline) = self.0 else {
            return;
        };
        let now = timeline.now();
        let x = |at: u64| {
            bounds.x
                + bounds.width
                    * (1.0 - now.saturating_sub(at) as f32 / WINDOW_MICROS as f32).clamp(0.0, 1.0)
        };
        let mut draw_interval = |key: usize, start: u64, end: u64| {
            if now.saturating_sub(end) > WINDOW_MICROS {
                return;
            }
            let color = if key < 2 {
                Color::from_rgb8(37, 99, 235)
            } else {
                Color::from_rgb8(139, 92, 246)
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: x(start),
                        y: bounds.y + key as f32 * 36.0 + 5.0,
                        width: (x(end) - x(start)).max(1.0),
                        height: 20.0,
                    },
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..iced::Border::default()
                    },
                    ..renderer::Quad::default()
                },
                color,
            );
        };
        for interval in &timeline.intervals {
            draw_interval(interval.key, interval.start, interval.end);
        }
        for (key, start) in timeline.held_since.iter().enumerate() {
            if let Some(start) = start {
                draw_interval(key, *start, now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_engine_output_and_switches_to_physical_only_when_requested() {
        let key = KeySlot::HorizontalFirst;
        let edge = MonitorEdge {
            key,
            pressed: true,
            synthetic: false,
        };
        let mut event = MonitorSnapshot {
            elapsed_micros: 50,
            filter_enabled: true,
            physical: Some(edge),
            outputs: vec![],
            decision: MonitorDecision::PressDelayed { delay_micros: 2000 },
        };
        let mut timeline = Timeline::default();
        let now = Instant::now();
        timeline.accept(event.clone(), false, now);
        assert!(
            !timeline.held(key),
            "a delayed physical press is not an output press"
        );
        event.elapsed_micros = 2050;
        event.physical = None;
        event.outputs.push(MonitorEdge {
            synthetic: true,
            ..edge
        });
        timeline.accept(event.clone(), false, now);
        assert!(timeline.held(key));
        timeline.clear();
        assert!(!timeline.held(key));
        event.outputs.clear();
        event.physical = Some(edge);
        event.filter_enabled = false;
        timeline.accept(event, false, now);
        assert!(timeline.held(key));
        assert!(timeline.physical);
    }
}
