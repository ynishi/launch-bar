//! Configuration module for Launch Bar

mod detect;
mod resolver;
mod state;
mod types;

pub use resolver::{PresetResolver, ResolvedConfig};
pub use state::AppState;
pub use types::{CommandConfig, Config, Preset, WindowSettings};
