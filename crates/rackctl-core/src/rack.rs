//! Rack layout: the rack, the devices in it and where each device is mounted.

use std::ops::RangeInclusive;

use kdl::{KdlDocument, KdlEntry, KdlNode, NodeKey};
use miette::SourceSpan;
use strum::{EnumString, IntoStaticStr, VariantNames};

use crate::catalog::{Catalog, Depth, Model, ModelError, Mount};
use crate::kdl_reader::{self, NodeReader, Problem, closest};
use crate::{IDENTIFIER_RULE, is_identifier};

/// The largest number of units a rack may have.
pub const MAX_UNITS: u8 = 60;

/// A rack and the devices mounted in it, as described in `rack.kdl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rack {
    /// Name of the rack, for example `homelab`.
    pub name: String,
    /// Height of the rack in units.
    pub units: u8,
    /// Devices in the order they appear in the file.
    pub devices: Vec<Device>,
}

/// A device mounted in a rack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Identifier used by the wiring and access files and on the command line.
    pub id: String,
    /// Identifier of the device's catalog model, for example `dell/r630-sff8`.
    pub model: String,
    /// Where the device is mounted.
    pub placement: Placement,
    /// Which side of the rack the device is mounted on.
    pub face: Face,
    /// Locations in the rack file, used to point at problems.
    pub spans: DeviceSpans,
}

/// Locations of a device's values in the rack file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceSpans {
    /// The whole `device` node.
    pub node: SourceSpan,
    /// The device id.
    pub id: SourceSpan,
    /// The `model` value.
    pub model: SourceSpan,
    /// The `u` value, when it is given.
    pub u: Option<SourceSpan>,
    /// The `mount` value, when it is given.
    pub mount: Option<SourceSpan>,
}

impl DeviceSpans {
    /// Returns where the device's position is given: its `u` value, or the `mount` value of
    /// a strip placed from the bottom of the rack.
    #[must_use]
    pub fn placement(&self) -> SourceSpan {
        self.u.or(self.mount).unwrap_or(self.node)
    }
}

/// Where a device is mounted. The number of units it covers comes from its catalog model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// In the rack's unit slots, where `u` is the lowest unit the device occupies.
    Slot { u: u8 },
    /// A vertical strip beside the rack, where `u` is the lowest unit the strip reaches.
    Strip { side: Side, u: u8 },
}

/// The side of the rack a strip is mounted on, as seen from the device's face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

/// The side of the rack a device is mounted on.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Face {
    #[default]
    Front,
    Rear,
}

/// The nodes a rack may contain, used to suggest corrections for misspelled ones.
const NODES: &[&str] = &["device"];

impl Rack {
    /// Parses a rack file.
    ///
    /// # Errors
    ///
    /// Returns every problem found in the file.
    pub fn parse(text: &str) -> Result<Self, Vec<Problem>> {
        let document = kdl_reader::parse(text)?;
        let mut problems = Vec::new();
        let mut rack_node = None;
        for node in document.nodes() {
            if node.name().value() == "rack" && rack_node.is_none() {
                rack_node = Some(node);
            } else {
                let problem = Problem::new(
                    format!("unexpected top-level node `{}`", node.name().value()),
                    node.name().span(),
                )
                .with_help("a rack file contains a single `rack { ... }` node");
                problems.push(problem);
            }
        }
        let Some(node) = rack_node else {
            problems.push(Problem::new(
                "the file does not contain a `rack { ... }` node",
                SourceSpan::from(0..0),
            ));
            return Err(problems);
        };
        match read_rack(node, &mut problems) {
            Some(rack) if problems.is_empty() => Ok(rack),
            _ => Err(problems),
        }
    }

    /// Checks the rack against the catalog: every model exists and is valid, strips use
    /// side-mounted models, every device fits in the rack and no two devices overlap.
    ///
    /// Problems inside a model's own file are not included; [`Catalog::model`] reports them.
    #[must_use]
    pub fn check(&self, catalog: &Catalog) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut slots: Vec<(&Device, &Model, RangeInclusive<u16>)> = Vec::new();
        for device in &self.devices {
            let Some(model) = find_model(device, catalog, &mut problems) else { continue };
            if let Some(problem) = mount_mismatch(device, model) {
                problems.push(problem);
                continue;
            }
            let units = device.units(model, self.units);
            if *units.end() > u16::from(self.units) {
                problems.push(
                    Problem::new(
                        format!(
                            "`{}` does not fit in the rack: it covers {}",
                            device.id,
                            describe_units(&units)
                        ),
                        device.spans.placement(),
                    )
                    .with_label(format!("the rack has {} units", self.units)),
                );
                continue;
            }
            if matches!(device.placement, Placement::Slot { .. }) {
                for (other, other_model, other_units) in &slots {
                    if let Some(problem) =
                        overlap((device, model, &units), (other, other_model, other_units))
                    {
                        problems.push(problem);
                    }
                }
                slots.push((device, model, units));
            }
        }
        problems
    }

    /// Returns the ranges of units that no device in the rack's slots covers, from the
    /// bottom of the rack up. Devices whose model is unknown or invalid are left out.
    #[must_use]
    pub fn free_units(&self, catalog: &Catalog) -> Vec<RangeInclusive<u16>> {
        let top = u16::from(self.units);
        let mut used = vec![false; usize::from(top) + 1];
        for device in &self.devices {
            if let (Placement::Slot { .. }, Ok(model)) =
                (device.placement, catalog.model(&device.model))
            {
                for u in device.units(model, self.units) {
                    if let Some(unit) = used.get_mut(usize::from(u)) {
                        *unit = true;
                    }
                }
            }
        }

        let mut free = Vec::new();
        let mut start = None;
        for u in 1..=top {
            match (used[usize::from(u)], start) {
                (false, None) => start = Some(u),
                (true, Some(first)) => {
                    free.push(first..=u - 1);
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(first) = start {
            free.push(first..=top);
        }
        free
    }
}

impl Device {
    /// Returns the units the device covers, from the lowest to the highest, given its
    /// catalog model and the height of the rack.
    ///
    /// A model without a height is one unit high, except for a strip, which then reaches
    /// the top of the rack.
    #[must_use]
    pub fn units(&self, model: &Model, rack_units: u8) -> RangeInclusive<u16> {
        let (u, height) = match self.placement {
            Placement::Slot { u } => (u, model.height.unwrap_or(1)),
            Placement::Strip { u, .. } => {
                (u, model.height.unwrap_or_else(|| rack_units.saturating_sub(u) + 1))
            }
        };
        let u = u16::from(u);
        u..=u + u16::from(height) - 1
    }
}

/// Looks up the device's model, reporting an unknown or invalid one.
fn find_model<'c>(
    device: &Device,
    catalog: &'c Catalog,
    problems: &mut Vec<Problem>,
) -> Option<&'c Model> {
    let problem = match catalog.model(&device.model) {
        Ok(model) => return Some(model),
        Err(ModelError::Unknown { suggestion, .. }) => {
            let problem =
                Problem::new(format!("unknown model `{}`", device.model), device.spans.model)
                    .with_label("not in the catalog");
            match suggestion {
                Some(close) => problem.with_help(format!("did you mean `{close}`?")),
                None => problem,
            }
        }
        Err(ModelError::Invalid(_)) => {
            Problem::new(format!("the model `{}` has problems", device.model), device.spans.model)
                .with_label("see the problems reported for its file")
        }
    };
    problems.push(problem);
    None
}

/// Reports a strip whose model is not side-mounted, or a side-mounted model placed in a slot.
fn mount_mismatch(device: &Device, model: &Model) -> Option<Problem> {
    match (device.placement, model.mount) {
        (Placement::Slot { .. }, Mount::Side) => Some(
            Problem::new(
                format!("`{}` is a strip mounted beside the rack", device.model),
                device.spans.model,
            )
            .with_label("side-mounted model")
            .with_help("add mount=\"left\" or mount=\"right\""),
        ),
        (Placement::Strip { .. }, Mount::Rack) => Some(
            Problem::new(
                format!("`{}` is mounted in the rack's slots, not beside it", device.model),
                device.spans.mount.unwrap_or(device.spans.node),
            )
            .with_label("needs a model with `mount \"side\"`")
            .with_help(if device.spans.u.is_some() {
                "remove `mount`"
            } else {
                "remove `mount` and give the lowest unit with u=N"
            }),
        ),
        _ => None,
    }
}

/// Reports two rack devices that share a unit. They may share it only when one is on the
/// front, the other on the rear, and both are half depth.
fn overlap(
    (device, model, units): (&Device, &Model, &RangeInclusive<u16>),
    (other, other_model, other_units): (&Device, &Model, &RangeInclusive<u16>),
) -> Option<Problem> {
    let shared = *units.start().max(other_units.start())..=*units.end().min(other_units.end());
    if shared.is_empty() {
        return None;
    }
    let half = model.depth == Depth::Half && other_model.depth == Depth::Half;
    if device.face != other.face && half {
        return None;
    }
    let problem = Problem::new(
        format!("`{}` overlaps `{}` on {}", device.id, other.id, describe_units(&shared)),
        device.spans.placement(),
    )
    .with_label(format!("`{}` covers {}", device.id, describe_units(units)))
    .with_label_at(
        other.spans.placement(),
        format!("`{}` covers {}", other.id, describe_units(other_units)),
    );
    Some(if device.face == other.face {
        problem
    } else {
        problem.with_help(
            "devices on the front and the rear can share units only when both models are \
             `depth \"half\"`",
        )
    })
}

/// Describes a range of units, for example `U2-U5` or `U7`.
fn describe_units(units: &RangeInclusive<u16>) -> String {
    if units.start() == units.end() {
        format!("U{}", units.start())
    } else {
        format!("U{}-U{}", units.start(), units.end())
    }
}

/// Reads the `rack { ... }` node. Returns `None` when a required value is missing.
fn read_rack(node: &KdlNode, problems: &mut Vec<Problem>) -> Option<Rack> {
    let mut reader = NodeReader::new(node, problems);
    let name = reader.arg_str(0, "name");
    let units = reader.req_int::<u8>("units");
    let block = reader.children();
    reader.finish();

    let units = match units {
        Some(units) if !(1..=MAX_UNITS).contains(&units) => {
            problems.push(Problem::new(
                format!("`units` must be between 1 and {MAX_UNITS}"),
                entry_span(node, "units"),
            ));
            None
        }
        units => units,
    };
    if name.as_deref().is_some_and(|name| !is_identifier(name)) {
        problems.push(Problem::new(format!("rack names {IDENTIFIER_RULE}"), entry_span(node, 0)));
    }

    let mut devices = Vec::new();
    let mut ids: Vec<(&str, SourceSpan)> = Vec::new();
    for child in block.map(KdlDocument::nodes).unwrap_or_default() {
        let key = child.name().value();
        if key != "device" {
            let mut problem =
                Problem::new(format!("unknown node `{key}` in a rack"), child.name().span());
            if let Some(close) = closest(key, NODES.iter().copied()) {
                problem = problem.with_help(format!("did you mean `{close}`?"));
            }
            problems.push(problem);
            continue;
        }
        // Ids are compared even for devices with other problems, so that a repeated id is
        // reported however many mistakes the devices have.
        if let Some(id) = child.entry(0).and_then(|entry| entry.value().as_string()) {
            let span = entry_span(child, 0);
            if let Some(&(_, first)) = ids.iter().find(|&&(other, _)| other == id) {
                problems.push(
                    Problem::new(format!("the device id `{id}` is used more than once"), span)
                        .with_label("used again here")
                        .with_label_at(first, "first used here"),
                );
            }
            ids.push((id, span));
        }
        devices.extend(read_device(child, units, problems));
    }

    Some(Rack { name: name?, units: units?, devices })
}

/// Reads a `device` node. `units` is the rack's height, when it is known.
fn read_device(node: &KdlNode, units: Option<u8>, problems: &mut Vec<Problem>) -> Option<Device> {
    let mut reader = NodeReader::new(node, problems);
    let id = reader.arg_str(0, "id");
    let model = reader.req_str("model");
    let u = reader.opt_int::<u8>("u");
    let side = reader.opt_enum::<Side>("mount");
    let face = reader.opt_enum::<Face>("face").unwrap_or_default();
    let u_misspelled = reader.has_misspelling_of("u");
    reader.finish();

    if id.as_deref().is_some_and(|id| !is_identifier(id)) {
        problems.push(Problem::new(format!("device ids {IDENTIFIER_RULE}"), entry_span(node, 0)));
    }
    if node.entry("u").is_none() && node.entry("mount").is_none() && !u_misspelled {
        problems.push(
            Problem::new("`device` is missing the property `u`", node.name().span())
                .with_label("add u=...")
                .with_help(
                    "use u=N for the lowest unit the device occupies, or mount=\"left\" or \
                     mount=\"right\" for a strip beside the rack",
                ),
        );
    }
    if u == Some(0) {
        problems.push(Problem::new("`u` must be at least 1", entry_span(node, "u")));
    }
    if let (Some(u), Some(units)) = (u, units)
        && u > units
    {
        problems.push(
            Problem::new(format!("`u` is above the top of the rack: {u}"), entry_span(node, "u"))
                .with_label(format!("the rack has {units} units")),
        );
    }

    let placement = match side {
        Some(side) => Placement::Strip { side, u: u.unwrap_or(1) },
        None => Placement::Slot { u: u? },
    };
    let spans = DeviceSpans {
        node: node.span(),
        id: entry_span(node, 0),
        model: entry_span(node, "model"),
        u: node.entry("u").map(KdlEntry::span),
        mount: node.entry("mount").map(KdlEntry::span),
    };
    Some(Device { id: id?, model: model?, placement, face, spans })
}

/// Returns the location of an argument or property of `node`, or of the node's name when
/// the entry is absent.
fn entry_span(node: &KdlNode, key: impl Into<NodeKey>) -> SourceSpan {
    node.entry(key).map_or_else(|| node.name().span(), KdlEntry::span)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use miette::Diagnostic;

    use super::*;

    fn messages(text: &str) -> Vec<String> {
        Rack::parse(text)
            .expect_err("the rack has problems")
            .iter()
            .map(|problem| problem.message().to_owned())
            .collect()
    }

    /// Models available to [`check`], as identifier and file contents.
    const MODELS: &[(&str, &str)] = &[
        ("x/server", r#"model { name "1U server"; kind "server" }"#),
        ("x/server-5u", r#"model { name "5U server"; kind "server"; height 5 }"#),
        ("x/switch", r#"model { name "Switch"; kind "switch"; depth "half" }"#),
        ("x/panel", r#"model { name "Panel"; kind "patch-panel"; depth "half" }"#),
        ("x/strip", r#"model { name "Strip"; kind "pdu"; mount "side"; height 20 }"#),
        ("x/strip-full", r#"model { name "Full strip"; kind "pdu"; mount "side" }"#),
        ("x/broken", r#"model { name "Broken" }"#),
    ];

    /// Returns a catalog of [`MODELS`], with the directory holding their files.
    fn test_catalog() -> (tempfile::TempDir, Catalog) {
        let dir = tempfile::tempdir().expect("temporary directory");
        for (id, text) in MODELS {
            let path = dir.path().join(format!("{id}.kdl"));
            fs::create_dir_all(path.parent().expect("file inside the directory")).expect("mkdir");
            fs::write(path, text).expect("write file");
        }
        let catalog = Catalog::open(&[dir.path()]).expect("readable catalog");
        (dir, catalog)
    }

    /// Places `devices` in a 36-unit rack and checks it against [`MODELS`].
    fn check(devices: &str) -> Vec<Problem> {
        let (_dir, catalog) = test_catalog();
        let rack =
            Rack::parse(&format!("rack \"r\" units=36 {{\n{devices}\n}}")).expect("valid rack");
        rack.check(&catalog)
    }

    fn check_messages(devices: &str) -> Vec<String> {
        check(devices).iter().map(|problem| problem.message().to_owned()).collect()
    }

    #[test]
    fn reports_overlapping_devices_with_both_labelled() {
        let problems = check(
            r#"device "a" model="x/server-5u" u=1
               device "b" model="x/server-5u" u=2"#,
        );
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message(), "`b` overlaps `a` on U2-U5");
        let labels: Vec<_> = problems[0]
            .labels()
            .expect("labels")
            .map(|label| label.label().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(labels, ["`b` covers U2-U6", "`a` covers U1-U5"]);
    }

    #[test]
    fn accepts_devices_that_touch() {
        let devices = r#"device "a" model="x/server-5u" u=1; device "b" model="x/server-5u" u=6"#;
        assert!(check(devices).is_empty());
    }

    #[test]
    fn reports_a_device_above_the_top_of_the_rack() {
        assert!(check(r#"device "a" model="x/server" u=36"#).is_empty());
        assert_eq!(
            check_messages(r#"device "a" model="x/server-5u" u=33"#),
            ["`a` does not fit in the rack: it covers U33-U37"]
        );
    }

    #[test]
    fn lets_half_depth_devices_share_a_unit_front_and_rear() {
        let devices =
            r#"device "a" model="x/switch" u=10; device "b" model="x/panel" u=10 face="rear""#;
        assert!(check(devices).is_empty());
    }

    #[test]
    fn reports_a_full_depth_device_sharing_a_unit_front_and_rear() {
        let problems = check(
            r#"device "a" model="x/server" u=10
               device "b" model="x/switch" u=10 face="rear""#,
        );
        assert_eq!(problems[0].message(), "`b` overlaps `a` on U10");
        assert!(problems[0].help().is_some_and(|help| help.contains("depth \"half\"")));
    }

    #[test]
    fn reports_half_depth_devices_sharing_a_unit_on_the_same_face() {
        let devices = r#"device "a" model="x/switch" u=10; device "b" model="x/panel" u=10"#;
        assert_eq!(check_messages(devices), ["`b` overlaps `a` on U10"]);
    }

    #[test]
    fn places_strips_beside_the_rack() {
        let devices = r#"
            device "a" model="x/server-5u" u=1
            device "pdu-a" model="x/strip" mount="left"
            device "pdu-b" model="x/strip" mount="left" u=17
            device "pdu-c" model="x/strip-full" mount="right" face="rear" u=19
        "#;
        assert!(check(devices).is_empty());
    }

    #[test]
    fn reports_a_strip_longer_than_the_rack() {
        assert_eq!(
            check_messages(r#"device "pdu" model="x/strip" mount="left" u=20"#),
            ["`pdu` does not fit in the rack: it covers U20-U39"]
        );
    }

    #[test]
    fn finds_the_free_units() {
        let (_dir, catalog) = test_catalog();
        let rack = Rack::parse(
            r#"rack "r" units=12 {
                device "a" model="x/server-5u" u=1
                device "b" model="x/switch" u=8
                device "c" model="x/panel" u=8 face="rear"
                device "pdu" model="x/strip-full" mount="left"
            }"#,
        )
        .expect("valid rack");
        assert_eq!(rack.free_units(&catalog), [6..=7, 9..=12]);
    }

    #[test]
    fn measures_the_units_a_device_covers() {
        let model = |text: &str| Model::parse("x/y", text).expect("valid model");
        let server = model(r#"model { name "S"; kind "server"; height 2 }"#);
        let strip = model(r#"model { name "P"; kind "pdu"; mount "side" }"#);
        let device = |placement| Device {
            id: "d".to_owned(),
            model: "x/y".to_owned(),
            placement,
            face: Face::Front,
            spans: DeviceSpans {
                node: SourceSpan::from(0..0),
                id: SourceSpan::from(0..0),
                model: SourceSpan::from(0..0),
                u: None,
                mount: None,
            },
        };
        assert_eq!(device(Placement::Slot { u: 28 }).units(&server, 36), 28..=29);
        assert_eq!(device(Placement::Strip { side: Side::Left, u: 19 }).units(&strip, 36), 19..=36);
    }

    #[test]
    fn reports_unknown_and_invalid_models() {
        let problems = check(
            r#"device "a" model="x/server-5" u=1
               device "b" model="x/broken" u=10"#,
        );
        assert_eq!(problems[0].message(), "unknown model `x/server-5`");
        assert_eq!(problems[0].help(), Some("did you mean `x/server-5u`?"));
        assert_eq!(problems[1].message(), "the model `x/broken` has problems");
    }

    #[test]
    fn reports_a_mount_that_does_not_match_the_model() {
        assert_eq!(
            check_messages(
                r#"device "a" model="x/strip" u=1
                   device "b" model="x/server" mount="right""#
            ),
            [
                "`x/strip` is a strip mounted beside the rack",
                "`x/server` is mounted in the rack's slots, not beside it",
            ]
        );
    }

    #[test]
    fn points_a_mount_mismatch_at_the_mount() {
        let text = r#"device "b" model="x/server" u=1 mount="left""#;
        let problems = check(text);
        let mount = text.find("mount=").expect("mount in the text");
        // The rack node and a newline come before the device in the checked text.
        let offset = "rack \"r\" units=36 {\n".len();
        assert_eq!(problems[0].span().offset(), offset + mount);
        assert_eq!(problems[0].help(), Some("remove `mount`"));
    }

    #[test]
    fn parses_a_rack() {
        let text = r#"
            rack "homelab" units=36 {
                device "er6p" model="ubiquiti/er6p" u=36
                device "r730" model="dell/r730-sff8" u=28 face="front"
                device "pdu-side" model="apc/ap7952" mount="right" face="rear"
            }
        "#;
        let rack = Rack::parse(text).expect("valid rack");
        assert_eq!(rack.name, "homelab");
        assert_eq!(rack.units, 36);
        let devices: Vec<_> = rack
            .devices
            .iter()
            .map(|d| (d.id.as_str(), d.model.as_str(), d.placement, d.face))
            .collect();
        assert_eq!(
            devices,
            [
                ("er6p", "ubiquiti/er6p", Placement::Slot { u: 36 }, Face::Front),
                ("r730", "dell/r730-sff8", Placement::Slot { u: 28 }, Face::Front),
                (
                    "pdu-side",
                    "apc/ap7952",
                    Placement::Strip { side: Side::Right, u: 1 },
                    Face::Rear
                ),
            ]
        );
    }

    #[test]
    fn places_a_strip_from_a_given_unit() {
        let text = r#"rack "r" units=36 { device "pdu" model="apc/ap7952" mount="left" u=19 }"#;
        let rack = Rack::parse(text).expect("valid rack");
        assert_eq!(rack.devices[0].placement, Placement::Strip { side: Side::Left, u: 19 });
    }

    #[test]
    fn accepts_an_empty_rack() {
        assert!(Rack::parse(r#"rack "r" units=42"#).expect("valid rack").devices.is_empty());
    }

    #[test]
    fn reports_a_missing_name_and_units() {
        assert_eq!(
            messages("rack {}"),
            ["`rack` is missing its name", "`rack` is missing the property `units`"]
        );
    }

    #[test]
    fn reports_units_out_of_range() {
        assert_eq!(messages(r#"rack "r" units=0"#), ["`units` must be between 1 and 60"]);
        assert_eq!(messages(r#"rack "r" units=61"#), ["`units` must be between 1 and 60"]);
    }

    #[test]
    fn reports_a_unit_outside_the_rack() {
        assert_eq!(
            messages(
                r#"rack "r" units=36 { device "a" model="x/a" u=0; device "b" model="x/b" u=37 }"#
            ),
            ["`u` must be at least 1", "`u` is above the top of the rack: 37"]
        );
    }

    #[test]
    fn reports_a_missing_unit_only_for_rack_slots() {
        assert_eq!(
            messages(r#"rack "r" units=36 { device "a" model="x/a" }"#),
            ["`device` is missing the property `u`"]
        );
        assert_eq!(
            messages(r#"rack "r" units=36 { device "a" model="x/a" mount="up" }"#),
            ["`mount` must be one of `left`, `right`, found `up`"]
        );
    }

    #[test]
    fn reports_invalid_and_repeated_ids() {
        assert_eq!(
            messages(
                r#"rack "r" units=36 {
                    device "Srv01" model="x/a" u=1
                    device "srv02" model="x/a" u=2
                    device "srv02" model="x/a" u=3
                }"#
            ),
            [
                format!("device ids {IDENTIFIER_RULE}"),
                "the device id `srv02` is used more than once".to_owned(),
            ]
        );
    }

    #[test]
    fn reports_ids_and_rack_names_that_break_the_rule() {
        assert_eq!(
            messages(r#"rack "-r" units=36 { device "srv-" model="x/a" u=1 }"#),
            [format!("rack names {IDENTIFIER_RULE}"), format!("device ids {IDENTIFIER_RULE}")]
        );
    }

    #[test]
    fn reports_a_repeated_id_even_when_the_other_device_has_problems() {
        assert_eq!(
            messages(
                r#"rack "r" units=36 {
                    device "a" u=1
                    device "a" model="x/a" u=2
                }"#
            ),
            [
                "`device` is missing the property `model`",
                "the device id `a` is used more than once"
            ]
        );
    }

    #[test]
    fn reports_a_misspelled_unit_once() {
        let problems = Rack::parse(r#"rack "r" units=36 { device "a" model="x/a" U=1 }"#)
            .expect_err("misspelled u");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message(), "unknown property `U` on `device`");
        assert_eq!(problems[0].help(), Some("did you mean `u`?"));
    }

    #[test]
    fn reports_misspelled_nodes_and_properties() {
        let problems = Rack::parse(
            r#"rack "r" units=36 { devce "a"; device "b" modle="x/b" model="x/b" u=1 }"#,
        )
        .expect_err("typos");
        let found: Vec<_> = problems.iter().map(|p| (p.message(), p.help())).collect();
        assert_eq!(
            found,
            [
                ("unknown node `devce` in a rack", Some("did you mean `device`?")),
                ("unknown property `modle` on `device`", Some("did you mean `model`?")),
            ]
        );
    }

    #[test]
    fn reports_invalid_faces() {
        let problems =
            Rack::parse(r#"rack "r" units=36 { device "a" model="x/a" u=1 face="rare" }"#)
                .expect_err("invalid face");
        assert_eq!(problems[0].message(), "`face` must be one of `front`, `rear`, found `rare`");
        assert_eq!(problems[0].help(), Some("did you mean `rear`?"));
    }

    #[test]
    fn reports_unexpected_top_level_nodes() {
        assert_eq!(
            messages("device \"x\""),
            [
                "unexpected top-level node `device`",
                "the file does not contain a `rack { ... }` node",
            ]
        );
        assert_eq!(
            messages(r#"rack "a" units=36; rack "b" units=42"#),
            ["unexpected top-level node `rack`"]
        );
    }
}
