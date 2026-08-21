#![forbid(unsafe_code)]

//! TokenZero implementation consumed directly by ZeroKernel.

#[path = "../../tokenzero-engine/src/zero_kernel.rs"]
mod implementation;

pub use implementation::ZeroTokenEngine;
