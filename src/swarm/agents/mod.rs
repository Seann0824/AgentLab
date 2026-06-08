// src/swarm/agents/mod.rs
// Agent 角色实现模块入口

pub mod coder;
pub mod common;
pub mod general;
pub mod memory;
pub mod researcher;
pub mod verifier;

pub use coder::CoderAgent;
pub use general::GeneralAgent;
pub use memory::MemoryAgent;
pub use researcher::ResearcherAgent;
pub use verifier::VerifierAgent;
