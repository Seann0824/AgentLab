pub mod types;
pub mod openai_compatible;
pub mod config;
pub mod manager;
pub mod providers;

pub use types::{ChatMessage, ModelEvent, ModelAdapter, ToolCall};
pub use openai_compatible::OpenAiCompatibleAdapter;
pub use config::ModelConfig;
pub use manager::ModelManager;
