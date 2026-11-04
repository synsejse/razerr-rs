use iced::{
    Color, Point, Rectangle, Renderer, Theme, alignment, mouse,
    widget::canvas::{
        self, Frame, Geometry, LineCap, LineDash, LineJoin, Path, Stroke, Text, gradient,
    },
};
use iced_t3::{color, font};

use crate::{curve, history::History};

const DAY: f64 = 86400.0;
const LEFT: f32 = 40.0;
const TOP: f32 = 26.0;
const RIGHT: f32 = 12.0;
const BOTTOM: f32 = 22.0;
const DASH: LineDash<'static> = LineDash {
    segments: &[3.0, 5.0],
    offset: 0,
};

/// The last day of battery readings: one line per session, so a gap in the
/// history stays a gap, colored by whether the device was charging.
pub struct Chart<'a>(pub &'a History);

/// `tint` at `alpha` over the panel behind the chart, as an opaque color. With
/// MSAA, an RX 9060 XT draws translucent gradient stops with a hard step.
fn over_panel(tint: Color, alpha: f32) -> Color {
    let blend = |panel: f32, tint: f32| panel + (tint - panel) * alpha;
    Color::from_rgb(
        blend(color::RAISED.r, tint.r),
        blend(color::RAISED.g, tint.g),
        blend(color::RAISED.b, tint.b),
    )
}

fn label(content: impl Into<String>, position: Point) -> Text {
    Text {
        content: content.into(),
        position,
        color: color::LABEL,
        size: 10.0.into(),
        font: font::REGULAR,
        ..Text::default()
    }
}

fn series(charging: bool) -> Color {
    if charging {
        color::SUCCESS
    } else {
        color::PRIMARY
    }
}

impl Chart<'_> {
    /// Sessions in time order; within one, timestamps strictly increase.
    fn sessions(&self) -> Vec<&[razerr_metrics::Point]> {
        self.0
            .points
            .chunk_by(connected)
            .filter(|session| !session.is_empty())
            .collect()
    }
}

impl<Message> canvas::Program<Message> for Chart<'_> {
    type State = ();
    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let plot = Plot {
            width: (bounds.width - LEFT - RIGHT).max(1.0),
            height: (bounds.height - TOP - BOTTOM).max(1.0),
            now: self.0.now,
        };
        // The fill comes first, so the grid stays visible on top of it.
        for session in self.sessions() {
            draw_fill(&mut frame, &plot, session);
        }
        draw_grid(&mut frame, &plot);
        for session in self.sessions() {
            draw_line(&mut frame, &plot, session);
        }
        draw_legend(&mut frame, bounds.width);
        vec![frame.into_geometry()]
    }
}

struct Plot {
    width: f32,
    height: f32,
    now: i64,
}

impl Plot {
    fn y(&self, percent: f32) -> f32 {
        TOP + self.height * (1.0 - percent / 100.0)
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "Chart coordinates are clamped to a 24-hour window and 0–100 percent"
    )]
    fn at(&self, point: &razerr_metrics::Point) -> (f32, f32) {
        let age = point
            .timestamp
            .saturating_sub(self.now.saturating_sub(86400)) as f64;
        (
            LEFT + self.width * (age / DAY).clamp(0.0, 1.0) as f32,
            self.y(point.battery.clamp(0.0, 100.0) as f32),
        )
    }
    /// The session's screen points, with any that share a pixel column dropped:
    /// slopes need x to increase.
    fn trace(&self, session: &[razerr_metrics::Point]) -> Vec<(f32, f32, bool)> {
        let mut trace: Vec<(f32, f32, bool)> = Vec::with_capacity(session.len());
        for point in session {
            let (x, y) = self.at(point);
            if trace.last().is_none_or(|last| x > last.0) {
                trace.push((x, y, point.charging));
            }
        }
        trace
    }
}

/// Adds the curve through the segments in `range` to a path being built.
fn segments(
    builder: &mut canvas::path::Builder,
    trace: &[(f32, f32, bool)],
    slopes: &[f32],
    range: std::ops::Range<usize>,
) {
    for index in range {
        let (from, to) = (trace[index], trace[index + 1]);
        let (first, second) = curve::controls(
            (from.0, from.1),
            (to.0, to.1),
            slopes[index],
            slopes[index + 1],
        );
        builder.bezier_curve_to(
            Point::new(first.0, first.1),
            Point::new(second.0, second.1),
            Point::new(to.0, to.1),
        );
    }
}

fn slopes_of(trace: &[(f32, f32, bool)]) -> Vec<f32> {
    curve::slopes(&trace.iter().map(|&(x, y, _)| (x, y)).collect::<Vec<_>>())
}

/// Where `trace` stays in one state, as ranges of its segments. A segment takes
/// the state of the reading that ends it.
fn runs(trace: &[(f32, f32, bool)]) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    for end in 1..trace.len() {
        if end + 1 == trace.len() || trace[end + 1].2 != trace[end].2 {
            runs.push(start..end);
            start = end;
        }
    }
    runs
}

fn draw_fill(frame: &mut Frame, plot: &Plot, session: &[razerr_metrics::Point]) {
    let trace = plot.trace(session);
    let slopes = slopes_of(&trace);
    let baseline = plot.y(0.0);
    for run in runs(&trace) {
        let (first, last) = (trace[run.start], trace[run.end]);
        let tint = series(last.2);
        let fade = gradient::Linear::new(Point::new(0.0, plot.y(100.0)), Point::new(0.0, baseline))
            .add_stop(0.0, over_panel(tint, 0.26))
            .add_stop(1.0, over_panel(tint, 0.02));
        frame.fill(
            &Path::new(|path| {
                path.move_to(Point::new(first.0, baseline));
                path.line_to(Point::new(first.0, first.1));
                segments(path, &trace, &slopes, run.clone());
                path.line_to(Point::new(last.0, baseline));
                path.close();
            }),
            fade,
        );
    }
}

fn draw_line(frame: &mut Frame, plot: &Plot, session: &[razerr_metrics::Point]) {
    let trace = plot.trace(session);
    if let [only] = trace[..] {
        // One reading makes no line, but it is still a reading.
        frame.fill(
            &Path::circle(Point::new(only.0, only.1), 2.5),
            series(only.2),
        );
    }
    let slopes = slopes_of(&trace);
    let line = Stroke {
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    for run in runs(&trace) {
        let start = trace[run.start];
        let tint = series(trace[run.end].2);
        let path = Path::new(|path| {
            path.move_to(Point::new(start.0, start.1));
            segments(path, &trace, &slopes, run.clone());
        });
        frame.stroke(&path, line.with_width(2.0).with_color(tint));
    }
}

fn draw_grid(frame: &mut Frame, plot: &Plot) {
    let right = LEFT + plot.width;
    for (percent, text) in [(100.0, "100%"), (50.0, "50%"), (0.0, "0%")] {
        let y = plot.y(percent);
        let stroke = Stroke::default().with_color(if percent == 0.0 {
            color::MUTED.scale_alpha(0.35)
        } else {
            color::HAIRLINE
        });
        frame.stroke(
            &Path::line(Point::new(LEFT, y), Point::new(right, y)),
            stroke,
        );
        frame.fill_text(Text {
            align_x: alignment::Horizontal::Right.into(),
            align_y: alignment::Vertical::Center,
            ..label(text, Point::new(LEFT - 7.0, y))
        });
    }
    let bottom = plot.y(0.0);
    let middle = LEFT + plot.width / 2.0;
    frame.stroke(
        &Path::line(
            Point::new(middle, plot.y(100.0)),
            Point::new(middle, bottom),
        ),
        Stroke {
            line_dash: DASH,
            ..Stroke::default().with_color(color::HAIRLINE)
        },
    );
    for (ratio, text, align) in [
        (0.0, "24 h ago", alignment::Horizontal::Left),
        (0.5, "12 h ago", alignment::Horizontal::Center),
        (1.0, "Now", alignment::Horizontal::Right),
    ] {
        frame.fill_text(Text {
            align_x: align.into(),
            ..label(text, Point::new(LEFT + plot.width * ratio, bottom + 7.0))
        });
    }
}

/// A key in the corner, so the two line colors explain themselves.
fn draw_legend(frame: &mut Frame, width: f32) {
    let mut right = width - RIGHT;
    for (charging, text, text_width) in [(true, "Charging", 42.0), (false, "On battery", 50.0)] {
        frame.fill_text(Text {
            align_x: alignment::Horizontal::Right.into(),
            align_y: alignment::Vertical::Center,
            ..label(text, Point::new(right, 8.0))
        });
        right -= text_width + 6.0;
        frame.fill(&Path::circle(Point::new(right, 8.0), 3.0), series(charging));
        right -= 16.0;
    }
}

fn connected(a: &razerr_metrics::Point, b: &razerr_metrics::Point) -> bool {
    a.session == b.session && b.timestamp > a.timestamp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(timestamp: i64, session: i64) -> razerr_metrics::Point {
        razerr_metrics::Point {
            timestamp,
            session,
            battery: 50.0,
            charging: false,
            poll_rate: 1000,
        }
    }

    #[test]
    fn chart_never_connects_sessions_or_nonincreasing_timestamps() {
        let a = point(100, 1);
        let mut b = a.clone();
        b.timestamp = 120;
        assert!(connected(&a, &b));
        b.session = 2;
        assert!(!connected(&a, &b));
        b.session = 1;
        b.timestamp = 100;
        assert!(!connected(&a, &b));
    }

    #[test]
    fn gradient_stops_stay_opaque_for_any_alpha() {
        for alpha in [0.0, 0.02, 0.26, 1.0] {
            assert!((over_panel(color::PRIMARY, alpha).a - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn runs_change_with_the_reading_that_ends_a_segment() {
        let trace = |states: &[bool]| -> Vec<(f32, f32, bool)> {
            states.iter().map(|state| (0.0, 0.0, *state)).collect()
        };
        let ends = |states: &[bool]| -> Vec<usize> {
            runs(&trace(states)).iter().map(|run| run.end).collect()
        };
        assert_eq!(ends(&[false]), Vec::<usize>::new());
        assert_eq!(ends(&[false, false, false]), [2]);
        assert_eq!(ends(&[false, false, true, true]), [1, 3]);
        assert_eq!(ends(&[false, true]), [1]);
    }

    #[test]
    fn sessions_split_where_the_history_breaks() {
        let history = History {
            points: vec![point(100, 1), point(120, 1), point(140, 2), point(140, 2)],
            ..History::default()
        };
        let sizes: Vec<_> = Chart(&history).sessions().iter().map(|s| s.len()).collect();
        assert_eq!(sizes, [2, 1, 1]);
    }

    #[test]
    fn points_clamped_to_the_edge_do_not_break_the_slopes() {
        let plot = Plot {
            width: 100.0,
            height: 100.0,
            now: 3 * 86400,
        };
        let trace = plot.trace(&[point(0, 1), point(1, 1), point(2 * 86400 + 1, 1)]);
        assert_eq!(trace.len(), 2, "older than the window, so on one column");
        assert!(slopes_of(&trace).iter().all(|slope| slope.is_finite()));
    }
}
