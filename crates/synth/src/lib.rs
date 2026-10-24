//! Deterministic synthetic definitions and objects.
//!
//! The same seed produces the same bytes. Tiers follow the phase 1 plan:
//! light is 50 attributes and 3 languages, medium 300 and 10, heavy 2000 and 20.
//! In every tier, 30% of attributes are language-dependent and a generated
//! object fills 60% of the attributes that apply to it.

mod definitions;
mod demo;
mod edits;
mod generate;
mod ids;
mod measure;
mod mutate;

pub use definitions::{ExportFormat, definitions, export};
pub use demo::{DemoSummary, write_demo};
pub use edits::{Edit, EditKind, edits};
pub use generate::{objects, objects_iter};
pub use measure::{SampleStats, TierMeasurement, measure};
pub use mutate::{ExpectedError, Mutation, all_mutations, apply_mutation, mutate};

/// How big a synthetic object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Light,
    Medium,
    Heavy,
}

impl Tier {
    pub const ALL: [Self; 3] = [Self::Light, Self::Medium, Self::Heavy];

    pub fn attribute_count(self) -> usize {
        match self {
            Self::Light => 50,
            Self::Medium => 300,
            Self::Heavy => 2000,
        }
    }

    pub fn language_count(self) -> usize {
        match self {
            Self::Light => 3,
            Self::Medium => 10,
            Self::Heavy => 20,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Medium => "medium",
            Self::Heavy => "heavy",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "light" => Some(Self::Light),
            "medium" => Some(Self::Medium),
            "heavy" => Some(Self::Heavy),
            _ => None,
        }
    }
}
