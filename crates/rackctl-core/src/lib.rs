//! Core library for rackctl.
//!
//! Provides the device catalog, the rack layout and configuration loading.
//! It has no dependencies on terminal rendering, networking or async runtimes.

pub mod catalog;
pub mod config;
pub mod kdl_reader;
pub mod rack;

/// Returns whether `name` is a valid identifier: it is not empty and uses only lowercase
/// letters, digits and `-`. Device identifiers and each part of a model identifier follow
/// this rule.
pub(crate) fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
