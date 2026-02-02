//! Configuration module for Launch Bar

mod detect;
mod state;
mod types;

pub use detect::detect_preset_idx;
pub use state::AppState;
pub use types::{CommandConfig, Config, Preset, WindowSettings};
