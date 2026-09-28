//! A sample status, to show a face without a device: most states appear somewhere.

use rackctl_core::catalog::{Face, Media, Model, Part, State};

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
    let elements = face.elements();
    let off = sample == Sample::Off;
    let group = |index: usize| {
        let entry = model.legend.get(elements[index].key);
        entry.and_then(|entry| entry.numbering.group.as_deref())
    };
    let look = |state, tone| Look { state, tone };
    (0..elements.len())
        .map(|index| {
            let element = &elements[index];
            let media = model.legend.get(element.key).and_then(|entry| entry.media);
            // The element's place among its part's numbers, counted from 0.
            let lowest = (0..elements.len())
                .filter(|&other| {
                    elements[other].part == element.part && group(other) == group(index)
                })
                .filter_map(|other| elements[other].number)
                .min();
            let at = element.number.zip(lowest).map_or(0, |(number, lowest)| number - lowest);
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
                    let combo = (0..elements.len()).any(|other| {
                        other != index
                            && elements[other].part == element.part
                            && elements[other].number == element.number
                            && group(other) == group(index)
                    });
                    let sfp = media == Some(Media::Sfp);
                    let idle =
                        combo && element.number.is_some_and(|number| (number % 2 == 1) != sfp);
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
