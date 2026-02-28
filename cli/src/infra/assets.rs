//! Infrastructure asset extraction — re-exports from `crate::assets`.
//!
//! Embedded asset extraction lives here in the infra layer.
//! The original `crate::assets` module re-exports from here for backward compat.

pub use crate::assets::{extract_assets, get_asset};
