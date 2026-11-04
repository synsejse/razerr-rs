//! The DPI vector pad: a sensitivity is a point (X DPI, Y DPI) in the plane.
//!
//! Both axes are logarithmic, so 400, 800 and 1600 stay visibly apart on a
//! 30,000 DPI sensor. Stages are drawn as numbered vectors; the one being
//! edited is the draggable handle. The X/Y sliders use the same scale.

use iced::{
    Point, Rectangle, Renderer, Theme, keyboard, mouse,
    widget::{
        Action,
        canvas::{self, Event, Frame, Geometry, Path, Stroke, Text, stroke},
        text::Alignment,
    },
};
use iced_t3::{color, font};

use crate::app::{Edit, Message};

pub const MIN_DPI: u16 = 100;
const TICKS: [u16; 9] = [100, 200, 400, 800, 1600, 3200, 6400, 12800, 25600];

/// Position of `value` on a logarithmic axis from `MIN_DPI` to `maximum`, in 0–1.
pub fn fraction(value: u16, maximum: u16) -> f32 {
    if maximum <= MIN_DPI {
        return 1.0;
    }
    let value = f32::from(value.clamp(MIN_DPI, maximum));
    ((value / f32::from(MIN_DPI)).ln() / (f32::from(maximum) / f32::from(MIN_DPI)).ln())
        .clamp(0.0, 1.0)
}

/// The DPI at a logarithmic position, rounded to a step of 50 like the C UI.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The result is clamped to MIN_DPI..=maximum, which fits u16"
)]
pub fn dpi_at(position: f32, maximum: u16) -> u16 {
    if maximum <= MIN_DPI {
        return maximum.max(1);
    }
    let ratio = f32::from(maximum) / f32::from(MIN_DPI);
    let raw = f32::from(MIN_DPI) * ratio.powf(position.clamp(0.0, 1.0));
    let stepped = (raw / 50.0).round() * 50.0;
    (stepped as u16).clamp(MIN_DPI, maximum)
}

/// Compact tick label: 400, 1.6k, 25.6k.
pub fn short(value: u16) -> String {
    if value >= 1000 {
        let thousands = f32::from(value) / 1000.0;
        if value.is_multiple_of(1000) {
            format!("{thousands:.0}k")
        } else {
            format!("{thousands:.1}k")
        }
    } else {
        value.to_string()
    }
}

pub struct Pad {
    pub maximum: u16,
    /// The vector being edited.
    pub target: (u16, u16),
    /// Every stage as (X, Y); `None` where a draft is not a number yet.
    pub stages: Vec<Option<(u16, u16)>>,
    /// Zero-based active stage.
    pub active: Option<usize>,
    /// Zero-based stage being edited, or `None` for the current DPI.
    pub editing: Option<usize>,
    pub linked: bool,
    pub enabled: bool,
}

#[derive(Default)]
pub struct State {
    dragging: bool,
}

struct Plot {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl Plot {
    fn new(bounds: Rectangle) -> Self {
        let (left, top, right, bottom) = (36.0, 22.0, 24.0, 26.0);
        Self {
            left,
            top,
            width: (bounds.width - left - right).max(1.0),
            height: (bounds.height - top - bottom).max(1.0),
        }
    }
    fn point(&self, fx: f32, fy: f32) -> Point {
        Point::new(
            self.left + self.width * fx,
            self.top + self.height * (1.0 - fy),
        )
    }
    fn fractions(&self, point: Point) -> (f32, f32) {
        (
            ((point.x - self.left) / self.width).clamp(0.0, 1.0),
            (1.0 - (point.y - self.top) / self.height).clamp(0.0, 1.0),
        )
    }
}

impl Pad {
    fn vector_at(&self, bounds: Rectangle, absolute: Point) -> (u16, u16) {
        let local = Point::new(absolute.x - bounds.x, absolute.y - bounds.y);
        let (mut fx, mut fy) = Plot::new(bounds).fractions(local);
        if self.linked {
            // Constrain to the X = Y diagonal: project onto it.
            let mean = f32::midpoint(fx, fy);
            fx = mean;
            fy = mean;
        }
        (dpi_at(fx, self.maximum), dpi_at(fy, self.maximum))
    }
    fn point_of(&self, plot: &Plot, (x, y): (u16, u16)) -> Point {
        plot.point(fraction(x, self.maximum), fraction(y, self.maximum))
    }

    fn draw_grid(&self, frame: &mut Frame, plot: &Plot) {
        let origin = plot.point(0.0, 0.0);
        let grid = Stroke::default()
            .with_color(color::HAIRLINE)
            .with_width(1.0);
        // Grid and logarithmic tick labels; label every other tick to avoid crowding.
        for (index, tick) in TICKS
            .iter()
            .copied()
            .filter(|tick| *tick <= self.maximum)
            .enumerate()
        {
            let f = fraction(tick, self.maximum);
            let x = plot.point(f, 0.0).x;
            let y = plot.point(0.0, f).y;
            frame.stroke(
                &Path::line(Point::new(x, plot.top), Point::new(x, origin.y)),
                grid,
            );
            frame.stroke(
                &Path::line(
                    Point::new(plot.left, y),
                    Point::new(origin.x + plot.width, y),
                ),
                grid,
            );
            if index % 2 == 0 {
                let label = |content: String, position: Point, align_x: Alignment| Text {
                    content,
                    position,
                    color: color::MUTED,
                    size: 10.0.into(),
                    font: font::REGULAR,
                    align_x,
                    align_y: iced::alignment::Vertical::Center,
                    ..Text::default()
                };
                frame.fill_text(label(
                    short(tick),
                    Point::new(x, origin.y + 13.0),
                    Alignment::Center,
                ));
                frame.fill_text(label(
                    short(tick),
                    Point::new(plot.left - 6.0, y),
                    Alignment::Right,
                ));
            }
        }
        // Axis names.
        for (content, position) in [
            ("X", Point::new(origin.x + plot.width + 13.0, origin.y)),
            ("Y", Point::new(plot.left, plot.top - 12.0)),
        ] {
            frame.fill_text(Text {
                content: content.into(),
                position,
                color: color::FOREGROUND,
                size: 11.0.into(),
                font: font::SEMIBOLD,
                align_x: Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }

        // The X = Y diagonal, where both axes share one sensitivity.
        frame.stroke(
            &Path::line(origin, plot.point(1.0, 1.0)),
            Stroke {
                line_dash: stroke::LineDash {
                    segments: &[4.0, 5.0],
                    offset: 0,
                },
                ..Stroke::default()
                    .with_color(if self.linked {
                        color::PRIMARY.scale_alpha(0.7)
                    } else {
                        color::MUTED.scale_alpha(0.5)
                    })
                    .with_width(1.0)
            },
        );
    }

    fn draw_stages(&self, frame: &mut Frame, plot: &Plot) {
        let origin = plot.point(0.0, 0.0);
        // Stage vectors that are not being edited.
        for (index, stage) in self.stages.iter().enumerate() {
            let Some(vector) = stage else { continue };
            if self.editing == Some(index) {
                continue;
            }
            let point = self.point_of(plot, *vector);
            let active = self.active == Some(index);
            frame.stroke(
                &Path::line(origin, point),
                Stroke::default()
                    .with_color(color::MUTED.scale_alpha(0.35))
                    .with_width(1.0),
            );
            frame.fill(
                &Path::circle(point, 9.0),
                if active {
                    color::PRIMARY.scale_alpha(0.18)
                } else {
                    color::RAISED
                },
            );
            frame.stroke(
                &Path::circle(point, 9.0),
                Stroke::default()
                    .with_color(if active {
                        color::PRIMARY
                    } else {
                        color::HAIRLINE
                    })
                    .with_width(1.5),
            );
            frame.fill_text(Text {
                content: (index + 1).to_string(),
                position: point,
                color: if active {
                    color::PRIMARY
                } else {
                    color::FOREGROUND
                },
                size: 10.0.into(),
                font: font::SEMIBOLD,
                align_x: Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }
    }

    fn draw_target(&self, frame: &mut Frame, plot: &Plot, dragging: bool) {
        let origin = plot.point(0.0, 0.0);
        // The edited vector, its projections and the handle.
        let handle = self.point_of(plot, self.target);
        let projection = Stroke {
            line_dash: stroke::LineDash {
                segments: &[2.0, 3.0],
                offset: 0,
            },
            ..Stroke::default()
                .with_color(color::PRIMARY.scale_alpha(0.45))
                .with_width(1.0)
        };
        frame.stroke(
            &Path::line(handle, Point::new(handle.x, origin.y)),
            projection,
        );
        frame.stroke(
            &Path::line(handle, Point::new(plot.left, handle.y)),
            projection,
        );
        frame.stroke(
            &Path::line(origin, handle),
            Stroke::default()
                .with_color(if self.enabled {
                    color::PRIMARY
                } else {
                    color::MUTED
                })
                .with_width(2.0)
                .with_line_cap(stroke::LineCap::Round),
        );
        let radius = if dragging { 9.0 } else { 7.5 };
        if dragging {
            frame.fill(
                &Path::circle(handle, 16.0),
                color::PRIMARY.scale_alpha(0.18),
            );
        }
        frame.fill(&Path::circle(handle, radius + 3.0), color::INSET);
        frame.fill(
            &Path::circle(handle, radius),
            if self.enabled {
                color::PRIMARY
            } else {
                color::MUTED
            },
        );
        if let Some(index) = self.editing {
            frame.fill_text(Text {
                content: (index + 1).to_string(),
                position: Point::new(handle.x + 14.0, handle.y - 12.0),
                color: color::PRIMARY,
                size: 11.0.into(),
                font: font::SEMIBOLD,
                align_x: Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }
    }
}

impl canvas::Program<Message> for Pad {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if self.enabled => {
                let position = cursor.position_over(bounds)?;
                state.dragging = true;
                let (x, y) = self.vector_at(bounds, position);
                Some(Action::publish(Message::Edit(Edit::Vector(x, y))).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                let (x, y) = self.vector_at(bounds, *position);
                Some(Action::publish(Message::Edit(Edit::Vector(x, y))).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                Some(Action::request_redraw().and_capture())
            }
            // The window's Escape handler restores the editor; just drop the drag.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if state.dragging => {
                state.dragging = false;
                None
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let plot = Plot::new(bounds);

        self.draw_grid(&mut frame, &plot);
        self.draw_stages(&mut frame, &plot);
        self.draw_target(&mut frame, &plot, state.dragging);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if self.enabled && cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logarithmic_axis_round_trips_common_values() {
        for value in [400, 800, 1600, 3200, 6400, 30000] {
            let position = fraction(value, 30000);
            let back = dpi_at(position, 30000);
            let error = (f32::from(back) - f32::from(value)).abs() / f32::from(value);
            assert!(error < 0.03, "{value} → {position} → {back}");
        }
        assert_eq!(dpi_at(0.0, 30000), MIN_DPI);
        assert_eq!(dpi_at(1.0, 30000), 30000);
    }

    #[test]
    fn nearby_low_values_stay_apart_on_a_large_range() {
        let a = fraction(400, 30000);
        let b = fraction(800, 30000);
        let c = fraction(1600, 30000);
        assert!(b - a > 0.1 && c - b > 0.1);
        assert!(
            ((b - a) - (c - b)).abs() < 1e-4,
            "equal ratios, equal spacing"
        );
    }

    #[test]
    fn degenerate_ranges_do_not_divide_by_zero() {
        assert!((fraction(100, 100) - 1.0).abs() < f32::EPSILON);
        assert_eq!(dpi_at(0.5, 100), 100);
        assert_eq!(dpi_at(0.5, 0), 1);
    }

    #[test]
    fn linked_drags_project_onto_the_diagonal() {
        let mut pad = Pad {
            maximum: 30000,
            target: (800, 800),
            stages: Vec::new(),
            active: None,
            editing: None,
            linked: true,
            enabled: true,
        };
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(300.0, 270.0));
        // Far right, near the bottom: independent axes keep X high and Y low.
        let corner = Point::new(250.0, 230.0);
        let (x, y) = pad.vector_at(bounds, corner);
        assert_eq!(x, y);
        pad.linked = false;
        let (x, y) = pad.vector_at(bounds, corner);
        assert!(x > 10000 && y < 200, "{x} × {y}");
    }

    #[test]
    fn tick_labels_are_compact() {
        assert_eq!(short(400), "400");
        assert_eq!(short(1600), "1.6k");
        assert_eq!(short(25600), "25.6k");
        assert_eq!(short(30000), "30k");
    }
}
