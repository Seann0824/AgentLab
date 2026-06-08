mod auto;
mod common;
mod pruning;
mod sliding;
mod truncate;

pub use auto::{auto_compress, force_compress};
pub use common::remove_orphaned_tool_messages;
pub use pruning::tool_call_pruning;
pub use sliding::sliding_window_mode;

use crate::context::{CompressResult, ContextMessage};

#[doc(hidden)]
pub fn _test_count_turns(messages: &[ContextMessage]) -> usize {
    common::count_turns(messages)
}

#[doc(hidden)]
pub fn _test_sliding_window_compress(
    messages: &mut Vec<ContextMessage>,
    max_turns: usize,
) -> CompressResult {
    sliding::sliding_window_compress(messages, max_turns)
}

#[cfg(test)]
mod tests;
