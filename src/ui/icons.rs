//! Procedural icons ported from CanvasIcon.tsx. Assets remain untouched.
use iced::{
    Color, Element, Length, Point, Radians, Rectangle, Size, Theme, Vector,
    advanced::{
        Layout, Renderer as _, Widget,
        graphics::geometry::Renderer as _,
        layout, mouse, renderer,
        widget::{Tree, tree},
    },
    widget::canvas::{Cache, Frame, LineCap, LineDash, LineJoin, Path, Stroke, path::Arc},
};
use std::{cell::Cell, f32::consts::PI};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Name {
    Star,
    Languages,
    Measurement,
    Chart,
    Keyboard,
    Restore,
    Check,
    Warning,
    Timer,
    Revert,
    Play,
    Stop,
    ArrowRight,
    ArrowForward,
    Target,
    ArrowLeft,
    ArrowUp,
    ArrowDown,
    ChevronLeft,
    ChevronRight,
    Layers,
    Edit,
    Power,
    Close,
}

pub fn icon<'a, Message: 'a>(name: Name, size: f32, color: Option<Color>) -> Element<'a, Message> {
    Element::new(Icon { name, size, color })
}
struct Icon {
    name: Name,
    size: f32,
    color: Option<Color>,
}
#[derive(Default)]
struct State {
    cache: Cache,
    key: Cell<Option<(Name, Color, Size)>>,
}
impl<Message> Widget<Message, Theme, iced::Renderer> for Icon {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(self.size.into(), self.size.into())
    }
    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.size, self.size)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let color = self.color.unwrap_or(style.text_color);
        // Rectangular icons use the existing quad renderer; curves use cached Canvas geometry.
        // The cache key includes the drawn size: geometry is baked in pixels,
        // so reusing one size's tessellation at another size softens edges.
        if self.name == Name::Stop {
            quad(renderer, bounds, color, 0.2, 0.2, 0.6, 0.6);
            return;
        }
        if self.name == Name::Chart {
            quad(renderer, bounds, color, 0.15, 0.8, 0.7, 0.1);
            for (x, h) in [(0.2095, 0.266), (0.423, 0.574), (0.6365, 0.406)] {
                quad(renderer, bounds, color, x, 0.85 - h, 0.154, h);
            }
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        // Inherit the parent's live text color, including hover, focus, and disabled button states.
        let size = bounds.size();
        if state.key.replace(Some((self.name, color, size))) != Some((self.name, color, size)) {
            state.cache.clear();
        }
        let geometry = state
            .cache
            .draw(renderer, size, |frame| draw_icon(frame, self.name, color));
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry)
        });
    }
}
/// The D-pad's center joystick tile: an 80x80 canvas tile with rounded-2xl corners,
/// a 48px dashed guide ring, an 8px resting center guide dot, and a dynamic moving dot
/// that shifts according to the winning direction (18px cardinal, 13px diagonal)
/// and lights up with an accent glow when active.
pub fn dpad_tile<'a, Message: 'a>(
    shift_x: f32,
    shift_y: f32,
    active: bool,
) -> Element<'a, Message> {
    Element::new(DpadTile {
        shift_x,
        shift_y,
        active,
    })
}

struct DpadTile {
    shift_x: f32,
    shift_y: f32,
    active: bool,
}

#[derive(Default)]
struct DpadTileState {
    cache: Cache,
    key: Cell<Option<(i32, i32, bool)>>,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for DpadTile {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DpadTileState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DpadTileState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(80.0.into(), 80.0.into())
    }

    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, 80.0, 80.0)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<DpadTileState>();
        let cache_key = (self.shift_x as i32, self.shift_y as i32, self.active);
        if state.key.replace(Some(cache_key)) != Some(cache_key) {
            state.cache.clear();
        }
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let size = bounds.size();
            let center = frame.center();

            // 1. Tile background & border (rounded-2xl, radius 16px)
            let tile_path = Path::new(|p| {
                p.rounded_rectangle(Point::ORIGIN, size, 16.0.into());
            });
            // bg-slate-100/90 (rgb 0xf1, 0xf5, 0xf9)
            frame.fill(&tile_path, Color::from_rgb8(0xf1, 0xf5, 0xf9));
            // border-slate-200/80 (rgb 0xe2, 0xe8, 0xf0)
            frame.stroke(
                &tile_path,
                Stroke::default()
                    .with_color(Color::from_rgb8(0xe2, 0xe8, 0xf0))
                    .with_width(1.0),
            );

            // 2. Dashed guide ring: 48px diameter (24px radius)
            let ring_radius = 24.0;
            frame.stroke(
                &Path::circle(center, ring_radius),
                Stroke {
                    line_dash: LineDash {
                        segments: &[3.0, 3.0],
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_color(Color::from_rgb8(0xe2, 0xe8, 0xf0))
                        .with_width(1.0)
                },
            );

            // 3. Center resting guide dot: 8px diameter (4px radius, slate-300/60)
            frame.fill(
                &Path::circle(center, 4.0),
                Color::from_rgba(0.80, 0.84, 0.88, 0.6),
            );

            // 4. Dynamic moving dot
            let dot_pos = Point::new(center.x + self.shift_x, center.y + self.shift_y);
            if self.active {
                // Outer ring glow (24px diameter / radius 12px)
                frame.fill(
                    &Path::circle(dot_pos, 12.0),
                    Color::from_rgba(0.23, 0.33, 0.91, 0.25),
                );
                // Shadow below active dot
                frame.fill(
                    &Path::circle(Point::new(dot_pos.x, dot_pos.y + 1.0), 10.0),
                    Color::from_rgba(0.23, 0.33, 0.91, 0.30),
                );
                // Active solid dot (20px diameter / radius 10px, #3a55e8)
                frame.fill(
                    &Path::circle(dot_pos, 10.0),
                    Color::from_rgb8(0x3a, 0x55, 0xe8),
                );
            } else {
                // Soft shadow below resting dot
                frame.fill(
                    &Path::circle(Point::new(dot_pos.x, dot_pos.y + 1.0), 9.0),
                    Color::from_rgba(0.0, 0.0, 0.0, 0.08),
                );
                // Resting solid dot (18px diameter / radius 9px, slate-400/80)
                frame.fill(
                    &Path::circle(dot_pos, 9.0),
                    Color::from_rgba(0.58, 0.64, 0.72, 0.8),
                );
            }
        });
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }
}

fn quad(
    renderer: &mut iced::Renderer,
    bounds: Rectangle,
    color: Color,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: bounds.x + x * bounds.width,
                y: bounds.y + y * bounds.height,
                width: w * bounds.width,
                height: h * bounds.height,
            },
            ..Default::default()
        },
        color,
    );
}
fn draw_icon(frame: &mut Frame, name: Name, color: Color) {
    let s = frame.width().min(frame.height());
    let stroke = Stroke::default()
        .with_color(color)
        .with_width((s * 0.1).max(1.2))
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round);
    let point = |x: f32, y: f32| Point::new(x * s, y * s);
    let line = |frame: &mut Frame, coords: &[(f32, f32)], closed: bool| {
        let path = Path::new(|p| {
            p.move_to(point(coords[0].0, coords[0].1));
            for &(x, y) in &coords[1..] {
                p.line_to(point(x, y));
            }
            if closed {
                p.close();
            }
        });
        frame.stroke(&path, stroke);
    };
    let arc = |frame: &mut Frame, x: f32, y: f32, r: f32, start: f32, end: f32| {
        frame.stroke(
            &Path::new(|p| {
                p.arc(Arc {
                    center: point(x, y),
                    radius: r * s,
                    start_angle: Radians(start),
                    end_angle: Radians(end),
                })
            }),
            stroke,
        );
    };
    match name {
        Name::Star => {
            let path = Path::new(|p| {
                for i in 0..10 {
                    let angle = -PI / 2.0 + i as f32 * PI / 5.0;
                    let r = if i % 2 == 0 { 0.45 } else { 0.2 };
                    let v = point(0.5 + angle.cos() * r, 0.5 + angle.sin() * r);
                    if i == 0 { p.move_to(v) } else { p.line_to(v) }
                }
                p.close();
            });
            frame.fill(&path, color);
        }
        Name::Measurement => line(
            frame,
            &[
                (0.15, 0.5),
                (0.3, 0.5),
                (0.42, 0.18),
                (0.6, 0.82),
                (0.72, 0.5),
                (0.85, 0.5),
            ],
            false,
        ),
        Name::Languages => {
            let parts: &[&[(f32, f32)]] = &[
                &[(8., 2.), (8., 5.)],
                &[(2., 5.), (14., 5.)],
                &[(4., 14.), (10., 8.), (12., 5.)],
                &[(5., 8.), (11., 14.)],
                &[(12., 22.), (17., 11.), (22., 22.)],
                &[(14., 18.), (20., 18.)],
            ];
            for part in parts {
                let p = Path::new(|p| {
                    p.move_to(point(part[0].0 / 24., part[0].1 / 24.));
                    for &(x, y) in &part[1..] {
                        p.line_to(point(x / 24., y / 24.));
                    }
                });
                frame.stroke(&p, stroke.with_width((s * 0.09).max(1.2)));
            }
        }
        Name::Keyboard => {
            frame.stroke(
                &Path::rectangle(point(0.15, 0.23), Size::new(s * 0.7, s * 0.55)),
                stroke,
            );
            line(frame, &[(0.32, 0.6), (0.68, 0.6)], false);
            for x in [0.28, 0.5, 0.72] {
                frame.fill(&Path::circle(point(x, 0.4), s * 0.04), color);
            }
        }
        Name::Restore => {
            arc(frame, 0.5, 0.5, 0.32, PI * 0.2, PI * 1.8);
            let x = 0.5 + (PI * 1.8).cos() * 0.32;
            let y = 0.5 + (PI * 1.8).sin() * 0.32;
            line(frame, &[(x - 0.15, y), (x, y), (x, y + 0.15)], false);
        }
        Name::Check => line(frame, &[(0.15, 0.52), (0.4, 0.78), (0.85, 0.25)], false),
        Name::Close => {
            line(frame, &[(0.15, 0.15), (0.85, 0.85)], false);
            line(frame, &[(0.85, 0.15), (0.15, 0.85)], false);
        }
        Name::Warning => {
            line(frame, &[(0.5, 0.15), (0.85, 0.85), (0.15, 0.85)], true);
            line(frame, &[(0.5, 0.38), (0.5, 0.62)], false);
            frame.fill(&Path::circle(point(0.5, 0.75), s * 0.05), color);
        }
        Name::Timer => {
            line(frame, &[(0.4, 0.15), (0.6, 0.15)], false);
            line(frame, &[(0.5, 0.15), (0.5, 0.2)], false);
            arc(frame, 0.5, 0.55, 0.35, 0., 2. * PI);
            line(frame, &[(0.5, 0.34), (0.5, 0.55), (0.6575, 0.55)], false);
        }
        Name::Revert => {
            let path = Path::new(|p| {
                p.move_to(point(0.85, 0.65));
                p.bezier_curve_to(point(0.85, 0.25), point(0.4, 0.25), point(0.25, 0.45));
            });
            frame.stroke(&path, stroke);
            line(frame, &[(0.25, 0.25), (0.15, 0.45), (0.4, 0.55)], false);
        }
        Name::Play => {
            // Reference insets the triangle by the 0.15 padding plus a
            // small margin, matching the canvas `p + s * 0.08` origin.
            frame.fill(
                &Path::new(|p| {
                    p.move_to(point(0.23, 0.15));
                    p.line_to(point(0.85, 0.5));
                    p.line_to(point(0.23, 0.85));
                    p.close();
                }),
                color,
            );
        }
        Name::ArrowRight | Name::ArrowForward => {
            let d = if name == Name::ArrowForward {
                0.25
            } else {
                0.28
            };
            line(frame, &[(0.15, 0.5), (0.85, 0.5)], false);
            line(
                frame,
                &[(0.85 - d, 0.5 - d), (0.85, 0.5), (0.85 - d, 0.5 + d)],
                false,
            );
        }
        Name::ArrowLeft => {
            line(frame, &[(0.85, 0.5), (0.15, 0.5)], false);
            line(frame, &[(0.43, 0.22), (0.15, 0.5), (0.43, 0.78)], false);
        }
        Name::ArrowUp => {
            line(frame, &[(0.5, 0.85), (0.5, 0.15)], false);
            line(frame, &[(0.22, 0.43), (0.5, 0.15), (0.78, 0.43)], false);
        }
        Name::ArrowDown => {
            line(frame, &[(0.5, 0.15), (0.5, 0.85)], false);
            line(frame, &[(0.22, 0.57), (0.5, 0.85), (0.78, 0.57)], false);
        }
        Name::ChevronLeft | Name::ChevronRight => {
            // Reference chevrons are tall: half-width 0.15, half-height
            // 0.38 around the center, stroked heavier than body icons.
            let (tip, base) = if name == Name::ChevronLeft {
                (0.35, 0.65)
            } else {
                (0.65, 0.35)
            };
            let p = Path::new(|p| {
                p.move_to(point(base, 0.12));
                p.line_to(point(tip, 0.5));
                p.line_to(point(base, 0.88));
            });
            frame.stroke(&p, stroke.with_width((s * 0.11).max(1.8)));
        }
        Name::Target => {
            // Reference target: outer ring at the padded radius with a
            // filled center at 35% of that radius.
            arc(frame, 0.5, 0.5, 0.35, 0., 2. * PI);
            frame.fill(&Path::circle(point(0.5, 0.5), s * 0.35 * 0.35), color);
        }
        Name::Layers => {
            let paths: &[(&[(f32, f32)], bool)] = &[
                (&[(0.5, 0.12), (0.88, 0.31), (0.5, 0.5), (0.12, 0.31)], true),
                (&[(0.12, 0.5), (0.5, 0.69), (0.88, 0.5)], false),
                (&[(0.12, 0.69), (0.5, 0.88), (0.88, 0.69)], false),
            ];
            for (coords, closed) in paths {
                let p = Path::new(|p| {
                    p.move_to(point(coords[0].0, coords[0].1));
                    for &(x, y) in &coords[1..] {
                        p.line_to(point(x, y));
                    }
                    if *closed {
                        p.close();
                    }
                });
                frame.stroke(&p, stroke.with_width((s * 0.09).max(1.2)));
            }
        }
        Name::Edit => line(
            frame,
            &[
                (0.2, 0.8),
                (0.2, 0.65),
                (0.65, 0.2),
                (0.8, 0.35),
                (0.35, 0.8),
            ],
            true,
        ),
        Name::Power => {
            arc(
                frame,
                0.5,
                0.56,
                0.3,
                -PI / 2. + 0.6,
                -PI / 2. - 0.6 + 2. * PI,
            );
            line(frame, &[(0.5, 0.14), (0.5, 0.56)], false);
        }
        Name::Stop | Name::Chart => unreachable!("rectangular icons draw through quads"),
    }
}
