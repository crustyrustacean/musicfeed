// src/routes.rs

pub mod health_check;
pub mod index;
pub mod rotation;
mod sse;
pub mod stats;

pub use health_check::*;
pub use index::*;
pub use rotation::*;
pub use stats::*;
