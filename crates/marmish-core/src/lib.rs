//! Offline configuration and opaque probe framing. No execution or networking.

mod config;
mod frame;

pub use config::{ConfigError, RelaySet, Retention};
pub use frame::{FrameError, Limits, ProbeEnvelope};
