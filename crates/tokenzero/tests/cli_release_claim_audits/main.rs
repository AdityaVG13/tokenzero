use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;


mod adapter;
mod artifact;
mod benchmark;
mod eval;
mod gates;
mod misc;
