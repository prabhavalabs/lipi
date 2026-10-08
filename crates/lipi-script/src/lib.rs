//! Script analysis for Sinhala, Tamil and Latin text.
//!
//! This crate holds the language-specific knowledge that the rest of lipi relies on:
//!
//! * [`script`]: per-script letter counts and the language they imply.
//! * [`health`]: checks that tell whether a PDF text layer can be trusted
//!   (legacy-font Latin "gibberish", vowel signs stored in visual order, foreign code points
//!   inside Indic words).
//! * [`repair`]: deterministic repair of visual-order vowel signs.
//! * [`normalize`]: Unicode normalisation that preserves the joiners Sinhala and Tamil need.
//! * [`fonts`]: recognition of legacy (non-Unicode) Sinhala and Tamil font families.

pub mod fonts;
pub mod health;
pub mod normalize;
pub mod repair;
pub mod script;

pub use health::{TextHealth, assess};
pub use normalize::normalize;
pub use script::{Lang, ScriptShares, Shares};
