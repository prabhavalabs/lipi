//! Document model, input format detection and output renderers.

pub mod format;
pub mod model;
pub mod render;

pub use format::{InputFormat, sniff};
pub use model::{Document, Method, Page};
pub use render::{OutputFormat, RenderOptions, render};
