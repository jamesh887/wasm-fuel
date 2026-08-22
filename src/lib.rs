//! A WebAssembly binary parser and a fuel-metered stack interpreter, in safe
//! Rust with zero dependencies. See the crate README for the full pitch and
//! the supported subset.

#![forbid(unsafe_code)]

pub mod leb;
pub mod module;
