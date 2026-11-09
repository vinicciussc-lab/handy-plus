//! Handy Plus additions: first-run import from Handy and text correction.

pub mod gpu_guard;
pub mod import;
pub mod local_engine;
#[allow(dead_code)] // consumed by the correction session (next task)
pub mod selection;
pub mod settings;
pub mod stream;
pub mod text;
