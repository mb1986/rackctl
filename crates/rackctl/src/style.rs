//! Text styles shared by the commands. Colours are left out automatically when the output
//! is not a terminal or `NO_COLOR` is set.

use anstyle::{AnsiColor, Style};

/// Row labels, such as `catalog` or `devices`.
pub const LABEL: Style = Style::new().bold();

/// Headings, such as the rack's name.
pub const HEADING: Style = Style::new().bold();

/// Details that support the main value, such as where models come from.
pub const NOTE: Style = Style::new().dimmed();

/// A successful result.
pub const OK: Style = AnsiColor::Green.on_default().bold();

/// A result with problems.
pub const ERROR: Style = AnsiColor::Red.on_default().bold();
