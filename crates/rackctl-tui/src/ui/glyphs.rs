//! Default glyphs, for elements whose legend entry gives none.

use rackctl_core::catalog::{Media, Part, State};

/// Returns the glyph of a part in a state.
#[must_use]
pub const fn default_glyph(part: Part, media: Option<Media>, state: State) -> &'static str {
    match (part, media, state) {
        (Part::Nic | Part::Mgmt | Part::Port, Some(Media::Sfp), State::Down) => "▭",
        (Part::Nic | Part::Mgmt | Part::Port, Some(Media::Sfp), _) => "▬",
        (Part::Nic | Part::Mgmt | Part::Port, _, State::Down) => "□",
        (Part::Nic | Part::Mgmt | Part::Port, ..) => "▣",
        (Part::Bay | Part::Outlet, ..) => "■",
        _ => "●",
    }
}
