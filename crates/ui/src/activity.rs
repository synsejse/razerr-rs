//! What the app is doing to a device, so that progress can appear on the control
//! being changed rather than on the app as a whole.

use std::time::{Duration, Instant};

use razerr_model::Serial;

use crate::state::Section;

/// How long a finished change keeps its check mark.
pub const CONFIRMATION: Duration = Duration::from_millis(1600);

/// The control a change belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Refresh,
    /// One-based DPI stage being activated.
    Stage(u8),
    Poll(u16),
    Apply(Section),
    Save,
    ClearHistory,
}

/// A change in flight, and what to say about it before and after.
#[derive(Clone, Debug)]
pub struct Activity {
    pub id: u64,
    pub control: Control,
    pub progress: String,
    pub done: String,
    /// The device it was sent to, if any.
    pub serial: Option<Serial>,
}

/// A change the device confirmed.
#[derive(Clone, Debug)]
pub struct Confirmed {
    pub control: Control,
    pub text: String,
    pub at: Instant,
}

impl Confirmed {
    pub fn expired(&self) -> bool {
        self.at.elapsed() >= CONFIRMATION
    }
}
