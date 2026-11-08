// src/routes.rs

pub mod health_check;
pub mod index;
pub mod rotation;
pub mod stats;

pub use health_check::*;
pub use index::*;
pub use rotation::*;
pub use stats::*;
