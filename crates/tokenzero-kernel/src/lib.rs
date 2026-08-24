#![forbid(unsafe_code)]

//! TokenZero implementation consumed directly by ZeroKernel.

#[path = "../../tokenzero-engine/src/zero_kernel.rs"]
mod implementation;

pub use implementation::{
    TokenizerIdPreflightError, UNLABELED_ESTIMATE_TOKENIZER_PREFIX, ZeroTokenEngine,
    preflight_tokenizer_id,
};
