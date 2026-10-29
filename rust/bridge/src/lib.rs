//! API exposed to the Flutter UI through flutter_rust_bridge.
//!
//! `frb_generated.rs` is produced by `flutter_rust_bridge_codegen generate`;
//! never edit it by hand. It is the only place in our crates where `unsafe`
//! code is allowed (FFI glue written by the generator).

pub mod api;

#[allow(unsafe_code, clippy::unwrap_used, clippy::expect_used)]
mod frb_generated;
