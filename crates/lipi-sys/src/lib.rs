//! System integration: hardware probe, performance profiles, resource governor, dependency
//! discovery and the one-time installer for OCR models and the PDF renderer.

pub mod deps;
pub mod governor;
pub mod hardware;
pub mod install;
pub mod manifest;
pub mod paths;
pub mod profile;

pub use hardware::Hardware;
pub use profile::Profile;
