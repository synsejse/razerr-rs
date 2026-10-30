pub const CONTROL_PATH: &str = "/org/razerr_rs/Control";

#[zbus::proxy(
    interface = "org.razerr_rs.Control1",
    default_service = "org.razerr_rs.Service",
    default_path = "/org/razerr_rs/Control"
)]
pub trait Control {
    fn open(&self) -> zbus::Result<()>;
    fn settings(&self) -> zbus::Result<()>;
    fn refresh(&self) -> zbus::Result<()>;
    fn clear_history(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    fn quit(&self) -> zbus::Result<()>;
    fn status(&self) -> zbus::Result<String>;
}
