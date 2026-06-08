// src/goal/mod.rs
//
// 🎯 目标驱动（Goal-Driven）能力 — 模块入口
//
// 提供 Goal-Driven 能力的统一导出，包含：
// - types: Goal, GoalStatus 等数据类型
// - registry: GoalRegistry 持久化存储
//
// 设计文档: docs/designs/goal-driven-capability.md

pub mod types;
pub mod registry;

pub use self::types::{Goal, GoalStatus};
pub use self::registry::{GoalRegistry, GoalIndexEntry};
