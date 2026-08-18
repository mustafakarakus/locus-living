//! `homeai-core` library. The binary is a thin `main` over this crate.
//!
//! Agents live under [`agents`] as modules. They are spawned as supervised
//! tokio tasks (UC-101). They never call each other; they publish/subscribe
//! `homeai.HomeEvent` on the in-process bus.

pub mod agents;
pub mod api;
pub mod bus;
pub mod db;
pub mod supervisor;
