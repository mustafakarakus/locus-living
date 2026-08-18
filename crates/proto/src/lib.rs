//! Wire types for `homeai.HomeEvent` and `NodeService`.
//!
//! Codegen from `proto/homeai.proto` lands with UC-102. Until then this crate
//! exists so `core` and `noded` can depend on a single place.

pub const SCHEMA_VERSION: u32 = 1;
