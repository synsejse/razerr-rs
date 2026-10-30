//! Basic iced views for the service-owned device model.

mod app;
mod chart;
mod dpi_pad;
mod edit;
mod history;
mod keyboard_button;
mod state;
mod style;
mod subscriptions;
mod view;

use razerr_storage::{Paths, Settings};
use tokio::sync::mpsc;

/// Startup state, resolved before the window takes ownership of the main thread.
#[derive(Clone)]
pub struct Options {
    pub paths: Paths,
    pub settings: Settings,
    pub dark_override: bool,
    pub show_settings: bool,
    pub initial_error: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Activation {
    Present,
    Settings,
    Dark,
    Quit,
}

/// Run the desktop window. D-Bus instance ownership stays with the caller.
pub fn run(options: Options, receiver: mpsc::Receiver<Activation>) -> iced::Result {
    let activations = subscriptions::Activations::new(receiver);
    let icon = razerr_images::decode(include_bytes!("../../../assets/icons/razerr-rs.png"))
        .ok()
        .and_then(|image| {
            iced::window::icon::from_rgba(image.pixels, image.width, image.height).ok()
        });
    iced::application(
        move || app::App::new(options.clone(), activations.clone()),
        app::App::update,
        app::App::view,
    )
    .title("razerr-rs")
    .theme(app::App::theme)
    .subscription(app::App::subscription)
    .settings(iced::Settings {
        default_font: style::REGULAR,
        ..iced::Settings::default()
    })
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .font(include_bytes!("../../../assets/fonts/inter/Inter-Regular.ttf").as_slice())
    .font(include_bytes!("../../../assets/fonts/inter/Inter-SemiBold.ttf").as_slice())
    .font(include_bytes!("../../../assets/fonts/lucide/Lucide.ttf").as_slice())
    .window(iced::window::Settings {
        size: iced::Size::new(1120.0, 780.0),
        min_size: Some(iced::Size::new(820.0, 560.0)),
        exit_on_close_request: false,
        icon,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "org.razerr_rs.App".into(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    })
    .run()
}
