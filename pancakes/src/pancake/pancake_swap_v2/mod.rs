//! PancakeSwap v2 (BSC) utilities using Alloy only.
//!
//! Scope: quoting & swapping (exact in), single-hop paths with explicit address arrays.

pub mod addresses;
pub mod path;
pub mod router;

pub use addresses::*;
pub use path::*;
pub use router::*;


