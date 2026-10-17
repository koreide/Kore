//! App-wide state: persisted storage, settings, navigation and resource data.
pub mod app_state;
pub mod ai_config;
pub mod ai_stream;
pub mod pins;
pub mod resource_item;
pub mod resources;
pub mod settings;
pub mod storage;
pub mod types;

pub use app_state::{AppEvent, AppState};
