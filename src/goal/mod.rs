// src/goal/mod.rs
//
// 🎯 目标驱动（Goal-Driven）能力 — 模块入口
//
// 提供 Goal-Driven 能力的统一导出，包含：
// - types: Goal, GoalStatus 等数据类型
// - registry: GoalRegistry 持久化存储
//
// 设计文档: docs/designs/goal-driven-capability.md

pub mod registry;
pub mod types;

pub use self::registry::{GoalIndexEntry, GoalRegistry};
pub use self::types::{Goal, GoalStatus};
