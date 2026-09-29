//! A sample status, to show a face without a device: most states appear somewhere.

use rackctl_core::catalog::{Face, Media, Model, Part, Slot, State};

use super::Look;
use crate::ui::theme::Tone;

/// The condition of a sample device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Sample {
    /// Running: a few ports down, bays empty or rebuilding.
    #[default]
    Normal,
    /// Switched off.
    Off,
}

/// Returns the look of each element of a sample device, by position in [`Face::elements`].
#[must_use]
pub fn sample_looks(model: &Model, face: &Face, sample: Sample) -> Vec<Look> {
    let off = sample == Sample::Off;
    let look = |state, tone| Look { state, tone };
    face.elements()
        .iter()
        .map(|element| {
            let entry = model.legend.get(element.key);
            let group = entry.and_then(|entry| entry.numbering.group.as_deref());
            let sfp = entry.and_then(|entry| entry.media) == Some(Media::Sfp);
            let table = face.numbers(element.part);
            // The element's place among its group's numbers, counted from 0, and its slot.
            let at = match (element.number, table.and_then(|table| table.base(group))) {
                (Some(number), Some(base)) => number - base,
                _ => 0,
            };
            let slot =
                table.zip(element.number).and_then(|(table, number)| table.get(group, number));
            let outlet_off = element.part == Part::Outlet && at % 4 == 2;
            match element.part {
                Part::Power | Part::Outlet if !off && !outlet_off => look(State::On, Tone::Good),
                Part::Psu if !off => look(State::Ok, Tone::Good),
                Part::Bay => {
                    let bays = model.components.bays;
                    if bays >= 4 && at + 2 >= bays {
                        look(State::Empty, Tone::Dim)
                    } else if off {
                        look(State::Ok, Tone::Dim)
                    } else if at == if bays <= 8 { 1 } else { 6 } {
                        look(State::Rebuilding, Tone::Warning)
                    } else {
                        look(State::Ok, Tone::Good)
                    }
                }
                Part::Nic | Part::Mgmt | Part::Port => {
                    // A combo uses its SFP cage on odd numbers, so its other connector is idle.
                    let idle = slot.is_some_and(Slot::is_combo)
                        && element.number.is_some_and(|number| (number % 2 == 1) != sfp);
                    if off || idle || at % 5 == 3 {
                        look(State::Down, Tone::Dim)
                    } else {
                        look(State::Up, if sfp { Tone::Link } else { Tone::Good })
                    }
                }
                _ => look(State::Off, Tone::Dim),
            }
        })
        .collect()
}
