use super::events::{RunEvent, RunEventKind};

/// Controls how agent activity is rendered to the outside world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Product default: keep assistant text visible and summarize tool activity.
    Concise,
    /// Debug-friendly terminal output with full tool stdout/stderr.
    Full,
    /// Newline-delimited JSON events for app shells and automation.
    Json,
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::Concise
    }
}

impl OutputMode {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Json)
    }

    pub fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Concise => "concise",
            Self::Full => "full",
            Self::Json => "json",
        }
    }
}

pub(super) fn emit_json_event(
    kind: RunEventKind,
    subject: impl Into<String>,
    attributes: serde_json::Value,
) {
    let event = RunEvent::new(kind, subject, attributes);
    if let Ok(line) = serde_json::to_string(&event) {
        println!("{}", line);
    }
}
