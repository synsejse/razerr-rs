//! The iced window over the service-owned device model, on the `iced_t3` design.

mod activity;
mod app;
mod chart;
mod curve;
mod demo;
mod dpi_pad;
mod edit;
mod editor;
mod history;
#[cfg(test)]
mod render_tests;
mod ring;
mod state;
mod subscriptions;
mod view;

use iced::{Size, Task, window};
use razerr_storage::{Paths, Settings};
use tokio::sync::mpsc;

/// Startup state, resolved before the window takes ownership of the main thread.
#[derive(Clone)]
pub struct Options {
    pub paths: Paths,
    pub settings: Settings,
    pub show_settings: bool,
    pub initial_error: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum Activation {
    Present,
    Settings,
    Quit,
}

/// Run the desktop window. D-Bus instance ownership stays with the caller.
pub fn run(options: Options, receiver: mpsc::Receiver<Activation>) -> iced::Result {
    let activations = subscriptions::Activations::new(receiver);
    launch(move || app::App::new(options.clone(), activations.clone()))
}

/// Run the window on made-up devices, without the service, D-Bus or any files.
pub fn run_demo() -> iced::Result {
    launch(|| (app::App::demo(), Task::none()))
}

fn launch(boot: impl Fn() -> (app::App, Task<app::Message>) + 'static) -> iced::Result {
    let icon = razerr_images::decode(view::ICON_PNG)
        .ok()
        .and_then(|image| window::icon::from_rgba(image.pixels, image.width, image.height).ok());
    iced::application(boot, app::App::update, app::App::view)
        .title("razerr-rs")
        .theme(|_: &app::App| iced_t3::theme())
        .subscription(app::App::subscription)
        .settings(iced_t3::settings_for("org.razerr_rs.App"))
        .font(iced_aw::ICED_AW_FONT_BYTES)
        .window(window::Settings {
            size: Size::new(1120.0, 780.0),
            min_size: Some(Size::new(820.0, 560.0)),
            exit_on_close_request: false,
            icon,
            // Matches the desktop entry, so the taskbar shows the app's icon.
            ..iced_t3::chrome::window_settings_for("org.razerr_rs.App")
        })
        .run()
}
