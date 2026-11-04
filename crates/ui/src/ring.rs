use iced::{
    Point, Rectangle, Renderer, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, Text, path::Arc, stroke},
        text::Alignment,
    },
};
use iced_t3::{color, font};

/// A static battery ring with the level in its center.
pub struct Ring {
    pub level: Option<f64>,
    pub charging: bool,
    pub stale: bool,
    pub low: f64,
}

impl<Message> canvas::Program<Message> for Ring {
    type State = ();

    #[expect(
        clippy::cast_possible_truncation,
        reason = "Validated percentage 0–100 is converted to an f32 angle"
    )]
    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let width = 7.0;
        let radius = bounds.width.min(bounds.height) / 2.0 - width;
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default().with_color(color::TRACK).with_width(width),
        );
        if let Some(level) = self.level {
            let color = if self.stale {
                color::MUTED
            } else if self.charging {
                color::SUCCESS
            } else if level <= 5.0 {
                color::DANGER
            } else if level <= self.low {
                color::WARNING
            } else {
                color::PRIMARY
            };
            let start = -std::f32::consts::FRAC_PI_2;
            let sweep = std::f32::consts::TAU * (level / 100.0).clamp(0.0, 1.0) as f32;
            if sweep > 0.0 {
                frame.stroke(
                    &Path::new(|builder| {
                        builder.arc(Arc {
                            center,
                            radius,
                            start_angle: start.into(),
                            end_angle: (start + sweep).into(),
                        });
                    }),
                    Stroke::default()
                        .with_color(color)
                        .with_width(width)
                        .with_line_cap(stroke::LineCap::Round),
                );
            }
        }
        frame.fill_text(Text {
            content: self
                .level
                .map_or_else(|| "—".into(), |level| format!("{level:.0}%")),
            position: Point::new(center.x, center.y - 4.0),
            color: color::FOREGROUND,
            size: 22.0.into(),
            font: font::SEMIBOLD,
            align_x: Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            ..Text::default()
        });
        frame.fill_text(Text {
            content: if self.charging {
                "Charging".into()
            } else {
                "Battery".into()
            },
            position: Point::new(center.x, center.y + 17.0),
            color: if self.charging {
                color::SUCCESS
            } else {
                color::MUTED
            },
            size: 11.0.into(),
            font: font::REGULAR,
            align_x: Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            ..Text::default()
        });
        vec![frame.into_geometry()]
    }
}
