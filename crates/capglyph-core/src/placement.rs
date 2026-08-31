//! Placement strategy for watermark embedding.

/// Watermark placement strategy (geometry-derived or PRNG/edge baselines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    /// Geometry-derived skeleton positions (default).
    #[default]
    Skeleton,
    /// Pseudorandom scatter (baseline / fallback).
    Prng,
    /// Edge-density (Sobel) baseline.
    Edge,
}

impl std::fmt::Display for Placement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skeleton => write!(f, "skeleton"),
            Self::Prng => write!(f, "prng"),
            Self::Edge => write!(f, "edge"),
        }
    }
}
