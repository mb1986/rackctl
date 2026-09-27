//! Core library for rackctl.
//!
//! Provides the device catalog, the rack layout and configuration loading.
//! It has no dependencies on terminal rendering, networking or async runtimes.

pub mod catalog;
pub mod config;
pub mod kdl_reader;
pub mod rack;

/// The rule for identifiers, worded to follow the kind of name in error messages, for
/// example "device ids may only use ...".
pub(crate) const IDENTIFIER_RULE: &str =
    "may only use lowercase letters, digits and `-`, and may not start or end with `-`";

/// Returns whether `name` is a valid identifier: it is not empty, uses only lowercase
/// letters, digits and `-`, and does not start or end with `-`. Rack names, device ids and
/// each part of a model id follow this rule.
pub(crate) fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_valid_identifiers() {
        for valid in ["ghost", "patch-32", "r630-sff8", "42u", "a"] {
            assert!(is_identifier(valid), "{valid}");
        }
        for invalid in ["", "-", "-a", "a-", "Ghost", "a_b", "a b", "a.b"] {
            assert!(!is_identifier(invalid), "{invalid}");
        }
    }
}
