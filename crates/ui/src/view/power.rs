use super::sensitivity::format_duration;
use crate::{
    app::{App, Edit, Message},
    chart::Chart,
    dpi_pad::Ring,
    history,
};
use iced::{
    Element, Fill,
    widget::{Column, canvas, column, container, row, slider},
};
use iced_t3::{cancellable, font, style, widget as t3};
use razerr_model::{Availability, Device};

impl App {
    pub(super) fn power<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        let mut page = Column::new().spacing(24);
        if device.battery.is_some() {
            page = page.push(t3::section(
                t3::section_title("Battery"),
                None,
                self.battery(device),
            ));
        }
        let mut rows = Vec::new();
        if device.idle_time.is_some() {
            rows.push(self.sleep_row());
        }
        if device.low_battery.is_some() {
            rows.push(self.warning_row());
        }
        if !rows.is_empty() {
            page = page.push(t3::section(t3::section_title("Power saving"), None, rows));
        }
        page.into()
    }

    fn battery<'a>(&'a self, device: &'a Device) -> Vec<Element<'a, Message>> {
        let battery = device.battery.as_ref();
        let stale = self.battery_stale(device);
        let ring = canvas(Ring {
            level: battery
                .and_then(|battery| battery.level.displayed())
                .map(|level| level.get()),
            charging: battery.is_some_and(|battery| battery.charging == Some(true)) && !stale,
            stale,
            low: device
                .low_battery
                .as_ref()
                .and_then(|reading| reading.displayed())
                .map_or(15.0, |value| f64::from(value.get())),
        })
        .width(112)
        .height(112);
        let mut summary = Column::new().spacing(4).width(Fill);
        summary = summary.push(t3::text(self.battery_summary(device), 14).font(font::MEDIUM));
        if !stale && let Some(prediction) = &self.history.prediction {
            summary = summary.push(t3::muted(history::details(Some(prediction))));
        }
        let mut rows: Vec<Element<'a, Message>> = vec![
            container(row![ring, summary].spacing(20).align_y(iced::Center))
                .padding([14, 16])
                .into(),
        ];
        rows.push(container(self.insights()).padding([14, 16]).into());
        rows
    }

    fn battery_stale(&self, device: &Device) -> bool {
        device
            .battery
            .as_ref()
            .is_none_or(|battery| battery.level.fresh().is_none())
            || !self.state.snapshot.reliable
            || device.availability != Availability::Responding
    }

    fn battery_summary(&self, device: &Device) -> String {
        if device
            .battery
            .as_ref()
            .is_some_and(|battery| battery.level.displayed().is_none())
        {
            "Battery unavailable".to_owned()
        } else if self.battery_stale(device) {
            "Cached reading".to_owned()
        } else {
            history::estimate(self.history.prediction.as_ref())
        }
    }

    fn insights(&self) -> Element<'_, Message> {
        let mut contents = column![].spacing(12);
        if let Some(error) = &self.history.error {
            contents = contents.push(t3::warning(format!("History unavailable: {error}")));
        }
        if self.history.points.is_empty() {
            contents = contents.push(t3::muted("No battery history yet"));
        } else {
            contents = contents.push(canvas(Chart(&self.history)).width(Fill).height(200));
        }
        contents.into()
    }

    fn sleep_row(&self) -> Element<'_, Message> {
        let enabled = self.state.controls_enabled();
        let seconds = self.state.editor.idle_seconds();
        value_row(
            "Sleep after",
            seconds.map_or_else(|| "Unavailable".to_owned(), format_duration),
            seconds.is_some(),
            cancellable(
                slider(
                    60_u16..=900,
                    seconds.unwrap_or(60).clamp(60, 900),
                    move |value| {
                        if enabled && seconds.is_some() {
                            Message::Edit(Edit::Idle(value.to_string()))
                        } else {
                            Message::Idle
                        }
                    },
                )
                .step(15_u16)
                .width(220)
                .height(22)
                .style(style::slider(enabled && seconds.is_some())),
            ),
        )
    }

    fn warning_row(&self) -> Element<'_, Message> {
        let enabled = self.state.controls_enabled();
        let percent = self.state.editor.threshold_percent();
        value_row(
            "Low battery warning",
            percent.map_or_else(|| "Unavailable".to_owned(), |percent| format!("{percent}%")),
            percent.is_some(),
            cancellable(
                slider(5_u8..=25, percent.unwrap_or(5).clamp(5, 25), move |value| {
                    if enabled && percent.is_some() {
                        Message::Edit(Edit::Threshold(value.to_string()))
                    } else {
                        Message::Idle
                    }
                })
                .width(220)
                .height(22)
                .style(style::slider(enabled && percent.is_some())),
            ),
        )
    }
}

/// A slider and its value. A setting that couldn't be read says so rather than
/// showing a plausible default.
fn value_row<'a>(
    title: &'a str,
    value: String,
    known: bool,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    let value = t3::text(value, 12).color(if known {
        iced_t3::color::FOREGROUND
    } else {
        iced_t3::color::MUTED
    });
    t3::setting_titled(
        title,
        row![control, container(value).align_right(76)]
            .spacing(12)
            .align_y(iced::Center),
    )
}
