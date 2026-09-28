//! Colours.

use ratatui::style::Color;

/// Background of a device's panel.
pub const PANEL: Color = Color::Indexed(235);
/// Text fields, such as the device's name.
pub const TEXT: Color = Color::Indexed(255);
/// Characters a face draws as they are, and number labels.
pub const LITERAL: Color = Color::Indexed(245);
/// A device's ears.
pub const EAR: Color = Color::Indexed(250);
/// The line under a device.
pub const UNDERLINE: Color = Color::Indexed(244);
/// Rails, screw holes and unit numbers.
pub const RACK: Color = Color::Indexed(240);

/// The colour of an element's state, chosen by whoever knows the device's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Green: on, ok or up.
    Good,
    /// Cyan: an SFP link that is up.
    Link,
    /// Amber: a warning, or a bay rebuilding.
    Warning,
    /// Red: critical or failed.
    Critical,
    /// Blue: identify.
    Identify,
    /// Gray: off, empty, down or unknown.
    Dim,
}

impl Tone {
    /// Returns the tone's colour.
    #[must_use]
    pub const fn color(self) -> Color {
        Color::Indexed(match self {
            Self::Good => 40,
            Self::Link => 44,
            Self::Warning => 214,
            Self::Critical => 160,
            Self::Identify => 33,
            Self::Dim => 240,
        })
    }
}
