use iced::{
    Point, Rectangle, Renderer, Theme, mouse,
    widget::canvas::{self, Frame, Geometry, Path, Stroke, Text},
};

use crate::history::History;

pub struct Chart<'a>(pub &'a History);

impl<Message> canvas::Program<Message> for Chart<'_> {
    type State = ();
    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let tokens = crate::style::tokens(theme);
        let left = 40.0;
        let top = 12.0;
        let width = (bounds.width - left - 12.0).max(1.0);
        let height = (bounds.height - 48.0).max(1.0);
        for percent in [0_u16, 50, 100] {
            let y = top + height * (1.0 - f32::from(percent) / 100.0);
            frame.stroke(
                &Path::line(Point::new(left, y), Point::new(left + width, y)),
                Stroke::default().with_color(tokens.line),
            );
            frame.fill_text(Text {
                content: format!("{percent}%"),
                position: Point::new(0.0, y - 7.0),
                color: tokens.muted,
                size: 11.0.into(),
                ..Text::default()
            });
        }
        for (label, ratio, offset) in [
            ("24 h ago", 0.0, 0.0),
            ("12 h ago", 0.5, 0.0),
            ("Now", 1.0, 25.0),
        ] {
            frame.fill_text(Text {
                content: label.into(),
                position: Point::new(left + width * ratio - offset, top + height + 12.0),
                color: tokens.muted,
                size: 11.0.into(),
                ..Text::default()
            });
        }
        for pair in self.0.points.windows(2) {
            if connected(&pair[0], &pair[1]) {
                let a = position(&pair[0], self.0.now, left, top, width, height);
                let b = position(&pair[1], self.0.now, left, top, width, height);
                let color = if pair[1].charging {
                    tokens.info
                } else {
                    tokens.accent
                };
                frame.stroke(
                    &Path::line(a, b),
                    Stroke::default().with_color(color).with_width(2.0),
                );
            }
        }
        for point in &self.0.points {
            frame.fill(
                &Path::circle(position(point, self.0.now, left, top, width, height), 2.0),
                if point.charging {
                    tokens.info
                } else {
                    tokens.accent
                },
            );
        }
        vec![frame.into_geometry()]
    }
}

fn connected(a: &razerr_metrics::Point, b: &razerr_metrics::Point) -> bool {
    a.session == b.session && b.timestamp > a.timestamp
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "Chart coordinates are clamped to a 24-hour window and 0–100 percent"
)]
fn position(
    point: &razerr_metrics::Point,
    now: i64,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
) -> Point {
    let x = (point.timestamp.saturating_sub(now.saturating_sub(86400)) as f64 / 86400.0)
        .clamp(0.0, 1.0) as f32;
    let y = (point.battery / 100.0).clamp(0.0, 1.0) as f32;
    Point::new(left + width * x, top + height * (1.0 - y))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chart_never_connects_sessions_or_nonincreasing_timestamps() {
        let a = razerr_metrics::Point {
            timestamp: 100,
            session: 1,
            battery: 50.0,
            charging: false,
            poll_rate: 1000,
        };
        let mut b = a.clone();
        b.timestamp = 120;
        assert!(connected(&a, &b));
        b.session = 2;
        assert!(!connected(&a, &b));
        b.session = 1;
        b.timestamp = 100;
        assert!(!connected(&a, &b));
    }
}
