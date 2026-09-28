//! Catalog models: descriptions of hardware, one per file.

use std::collections::HashSet;

use kdl::{KdlDocument, KdlNode};
use strum::{EnumString, IntoStaticStr, VariantNames};

use super::face::{Faces, check_faces, check_glyphs, check_unused_keys, cut_faces, read_face};
use super::legend::{Legend, read_legend};
use crate::kdl_reader::{self, NodeReader, Problem};

/// A hardware model from the catalog, such as a particular server, switch or PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Identifier derived from the file path, for example `dell/r630-sff8`.
    pub id: String,
    /// Full product name, for example `Dell PowerEdge R630`.
    pub name: String,
    /// Name used where space is limited. Defaults to the full name.
    pub short: String,
    /// Optional free-text description.
    pub description: Option<String>,
    /// What kind of device the model is.
    pub kind: Kind,
    /// Height in rack units. When absent, a rack device is one unit high and a side
    /// strip spans the whole rack.
    pub height: Option<u8>,
    /// Where the device is mounted.
    pub mount: Mount,
    /// How deep the device is. Two half-depth devices can share a unit, one on the front
    /// and one on the rear.
    pub depth: Depth,
    /// How the device's mounting ears are drawn.
    pub ears: Ears,
    /// How many bays, power supplies, ports and other parts the device has.
    pub components: Components,
    /// The pictures the model's front panel is drawn from.
    pub faces: Faces,
    /// What the characters of the model's faces stand for.
    pub legend: Legend,
}

/// The kind of device a model describes. Catalog files write it in kebab-case, for
/// example `patch-panel`. Kinds are ordered as declared.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    EnumString,
    IntoStaticStr,
    VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Kind {
    Server,
    Storage,
    Tape,
    Kvm,
    Switch,
    Router,
    PatchPanel,
    Pdu,
    Ups,
    Blank,
    Shelf,
    Generic,
}

/// Where a device is mounted.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Mount {
    /// In the rack's unit slots.
    #[default]
    Rack,
    /// Vertically along the side of the rack, like a zero-unit PDU.
    Side,
}

/// How deep a device is, compared with the rack.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Depth {
    /// Takes the full depth of the rack, like most servers.
    #[default]
    Full,
    /// Takes at most half the depth of the rack, like patch panels and most switches.
    Half,
}

/// How a device's mounting ears are drawn.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Ears {
    /// Heavy frame corners, the usual look.
    #[default]
    Heavy,
    /// Screw heads only, for flat plates such as blank panels.
    Screws,
}

/// The number of each kind of part a device has. Parts that are not declared count as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Components {
    /// Drive bays.
    pub bays: u16,
    /// Power supplies.
    pub psus: u16,
    /// Network interfaces of a server.
    pub nics: u16,
    /// Dedicated management ports, such as a server's iDRAC or iLO port.
    pub mgmt: u16,
    /// RJ45 ports of a switch, router or patch panel.
    pub ports: u16,
    /// SFP cages of a switch or router.
    pub sfps: u16,
    /// Power outlets of a PDU or UPS.
    pub outlets: u16,
}

/// The nodes a model may contain, used to suggest corrections for misspelled ones.
const NODES: &[&str] = &[
    "name",
    "short",
    "description",
    "kind",
    "height",
    "mount",
    "depth",
    "ears",
    "bays",
    "psus",
    "nics",
    "mgmt",
    "ports",
    "sfps",
    "outlets",
    "face",
    "legend",
];

/// Nodes that may appear more than once: a model can have a normal, a compact and a
/// strip face.
const REPEATABLE: &[&str] = &["face"];

impl Model {
    /// Parses a model file.
    ///
    /// `id` is the model's catalog identifier and `text` the contents of its file.
    ///
    /// # Errors
    ///
    /// Returns every problem found in the file.
    pub fn parse(id: &str, text: &str) -> Result<Self, Vec<Problem>> {
        kdl_reader::parse_single(text, "model", |node, problems| read_model(id, node, problems))
    }
}

/// Reads the `model { ... }` node. Returns `None` when a required value is missing.
fn read_model(id: &str, node: &KdlNode, problems: &mut Vec<Problem>) -> Option<Model> {
    let mut reader = NodeReader::new(node, problems);
    let block = reader.children();
    reader.finish();

    let mut name = None;
    let mut short = None;
    let mut description = None;
    let mut kind = None;
    let mut height = None;
    let mut mount = Mount::default();
    let mut depth = Depth::default();
    let mut ears = Ears::default();
    let mut components = Components::default();
    let mut faces = Faces::default();
    let mut legend = Legend::default();
    let mut legend_span = None;
    let mut seen = HashSet::new();

    for child in block.map(KdlDocument::nodes).unwrap_or_default() {
        let key = child.name().value();
        if !seen.insert(key) && NODES.contains(&key) && !REPEATABLE.contains(&key) {
            problems.push(Problem::new(
                format!("`{key}` is given more than once"),
                child.name().span(),
            ));
        }

        match key {
            "name" => name = single(child, problems, |n| n.arg_str(0, "name")),
            "short" => short = single(child, problems, |n| n.arg_str(0, "short name")),
            "description" => {
                description = single(child, problems, |n| n.arg_str(0, "description"));
            }
            "kind" => kind = single(child, problems, |n| n.arg_enum(0, "kind")),
            "height" => {
                height = single(child, problems, |n| n.arg_int::<u8>(0, "height"));
                if height == Some(0) {
                    problems.push(Problem::new("`height` must be at least 1", child.span()));
                }
            }
            "mount" => {
                mount = single(child, problems, |n| n.arg_enum(0, "mount")).unwrap_or_default();
            }
            "depth" => {
                depth = single(child, problems, |n| n.arg_enum(0, "depth")).unwrap_or_default();
            }
            "ears" => ears = single(child, problems, |n| n.arg_enum(0, "ears")).unwrap_or_default(),
            "bays" => components.bays = count(child, problems),
            "psus" => components.psus = count(child, problems),
            "nics" => components.nics = count(child, problems),
            "mgmt" => components.mgmt = count(child, problems),
            "ports" => components.ports = count(child, problems),
            "sfps" => components.sfps = count(child, problems),
            "outlets" => components.outlets = count(child, problems),
            "face" => read_face(child, &mut faces, problems),
            "legend" => {
                legend = read_legend(child, problems);
                legend_span = Some(child.name().span());
            }
            _ => problems.push(
                Problem::new(format!("unknown node `{key}` in a model"), child.name().span())
                    .with_suggestion(key, NODES.iter().copied()),
            ),
        }
    }

    check_faces(&mut faces, mount, height, problems);
    cut_faces(&mut faces, &legend, problems);
    check_glyphs(&faces, &legend, problems);
    // A face that is present but invalid has already been reported.
    if let Some(span) = legend_span.filter(|_| !seen.contains("face")) {
        problems.push(
            Problem::new("a `legend` needs a `face` to describe", span)
                .with_help("add a `face` or remove the `legend`"),
        );
    }
    // A node that is present but invalid has already been reported.
    for key in ["name", "kind"] {
        if !seen.contains(&key) {
            problems.push(
                Problem::new(format!("the model is missing `{key}`"), node.name().span())
                    .with_label(format!("add `{key}` inside this block")),
            );
        }
    }
    // With other problems, such as two normal faces, it is not known which keys are meant to
    // be used, so unused keys are only reported for a model that is otherwise valid.
    if problems.is_empty() {
        check_unused_keys(&faces, &legend, problems);
    }

    let name = name?;
    Some(Model {
        id: id.to_owned(),
        short: short.unwrap_or_else(|| name.clone()),
        name,
        description,
        kind: kind?,
        height,
        mount,
        depth,
        ears,
        components,
        faces,
        legend,
    })
}

/// Reads a node holding a single value, such as `height 2`, and reports anything else
/// found on it.
fn single<T>(
    node: &KdlNode,
    problems: &mut Vec<Problem>,
    read: impl FnOnce(&mut NodeReader<'_, '_>) -> Option<T>,
) -> Option<T> {
    let mut reader = NodeReader::new(node, problems);
    let value = read(&mut reader);
    reader.finish();
    value
}

/// Reads a component count, such as `bays 8`. An invalid count is reported and read as zero.
fn count(node: &KdlNode, problems: &mut Vec<Problem>) -> u16 {
    let what = format!("number of {}", node.name().value());
    single(node, problems, |n| n.arg_int(0, &what)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(text: &str) -> Vec<String> {
        Model::parse("test/model", text)
            .expect_err("the model has problems")
            .iter()
            .map(|problem| problem.message().to_owned())
            .collect()
    }

    #[test]
    fn parses_a_complete_model() {
        let text = r##"
            model {
                name "Dell PowerEdge R630"; short "R630"
                description "1U server with eight 2.5-inch bays"
                kind "server"; height 1; bays 8; psus 2; nics 4; mgmt 1
                face #"""
                    p nnnnnnn
                    s s
                    """#
                legend {
                    p power
                    n name
                    s psu
                }
            }
        "##;
        let model = Model::parse("dell/r630-sff8", text).expect("valid model");
        assert_eq!(model.id, "dell/r630-sff8");
        assert_eq!(model.name, "Dell PowerEdge R630");
        assert_eq!(model.short, "R630");
        assert_eq!(model.description.as_deref(), Some("1U server with eight 2.5-inch bays"));
        assert_eq!(model.kind, Kind::Server);
        assert_eq!(model.height, Some(1));
        assert_eq!(model.mount, Mount::Rack);
        assert_eq!(model.depth, Depth::Full);
        assert_eq!(model.ears, Ears::Heavy);
        assert_eq!(
            model.components,
            Components { bays: 8, psus: 2, nics: 4, mgmt: 1, ..Components::default() }
        );
    }

    #[test]
    fn applies_defaults() {
        let model =
            Model::parse("generic/blank-1u", r#"model { name "Blank panel"; kind "blank" }"#)
                .expect("valid model");
        assert_eq!(model.short, "Blank panel");
        assert_eq!(model.description, None);
        assert_eq!(model.height, None);
        assert_eq!(model.mount, Mount::Rack);
        assert_eq!(model.depth, Depth::Full);
        assert_eq!(model.ears, Ears::Heavy);
        assert_eq!(model.components, Components::default());
    }

    #[test]
    fn reads_a_side_mounted_strip() {
        let text = r#"model { name "APC AP7952"; kind "pdu"; mount "side"; outlets 24 }"#;
        let model = Model::parse("apc/ap7952", text).expect("valid model");
        assert_eq!(model.mount, Mount::Side);
        assert_eq!(model.components.outlets, 24);
    }

    #[test]
    fn reads_a_half_depth_model() {
        let text = r#"model { name "Patch panel"; kind "patch-panel"; depth "half" }"#;
        let model = Model::parse("generic/patchpanel-24", text).expect("valid model");
        assert_eq!(model.depth, Depth::Half);
    }

    #[test]
    fn reports_an_invalid_depth_with_a_suggestion() {
        let problems = Model::parse("x/y", r#"model { name "X"; kind "switch"; depth "hlaf" }"#)
            .expect_err("invalid depth");
        assert_eq!(problems[0].message(), "`depth` must be one of `full`, `half`, found `hlaf`");
        assert_eq!(problems[0].help(), Some("did you mean `half`?"));
    }

    #[test]
    fn reads_screw_ears() {
        let text = r#"model { name "Blank panel"; kind "blank"; ears "screws" }"#;
        assert_eq!(Model::parse("generic/blank-1u", text).expect("valid model").ears, Ears::Screws);
    }

    #[test]
    fn reports_missing_name_and_kind() {
        assert_eq!(
            messages("model { height 1 }"),
            ["the model is missing `name`", "the model is missing `kind`"]
        );
    }

    #[test]
    fn reports_an_unknown_kind_with_a_suggestion() {
        let problems =
            Model::parse("x/y", r#"model { name "X"; kind "swich" }"#).expect_err("invalid kind");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message().starts_with("`kind` must be one of `server`, "));
        assert_eq!(problems[0].help(), Some("did you mean `switch`?"));
    }

    #[test]
    fn reports_each_mistake_once() {
        assert_eq!(
            messages(r#"model { name; kind "server"; foo 1; foo 2 }"#),
            [
                "`name` is missing its name",
                "unknown node `foo` in a model",
                "unknown node `foo` in a model",
            ]
        );
    }

    #[test]
    fn reports_an_unknown_node_with_a_suggestion() {
        let problems = Model::parse("x/y", r#"model { name "X"; kind "server"; heigth 2 }"#)
            .expect_err("typo");
        assert_eq!(problems[0].message(), "unknown node `heigth` in a model");
        assert_eq!(problems[0].help(), Some("did you mean `height`?"));
    }

    #[test]
    fn reports_repeated_nodes_and_invalid_values() {
        assert_eq!(
            messages(
                r#"model { name "X"; name "Y"; kind "server"; height 0; bays "8"; psus 2 3 }"#
            ),
            [
                "`name` is given more than once",
                "`height` must be at least 1",
                "`number of bays` must be a whole number, found the string \"8\"",
                "unexpected argument on `psus`",
            ]
        );
    }

    #[test]
    fn allows_a_normal_and_a_compact_face() {
        let text = r#"model { name "X"; kind "server"; face "a\nb"; face "c" compact=#true }"#;
        let faces = Model::parse("x/y", text).expect("valid model").faces;
        assert!(faces.normal.is_some() && faces.compact.is_some());
    }

    #[test]
    fn reports_a_legend_without_a_face() {
        assert_eq!(
            messages(r#"model { name "X"; kind "server"; legend { p power } }"#),
            ["a `legend` needs a `face` to describe"]
        );
    }

    #[test]
    fn reports_unexpected_top_level_nodes() {
        assert_eq!(
            messages("device \"x\""),
            [
                "unexpected top-level node `device`",
                "the file does not contain a `model { ... }` node",
            ]
        );
        assert_eq!(
            messages(r#"model { name "X"; kind "server" }; model { name "Y"; kind "server" }"#),
            ["unexpected top-level node `model`"]
        );
    }

    #[test]
    fn writes_kinds_in_kebab_case() {
        assert_eq!("patch-panel".parse(), Ok(Kind::PatchPanel));
        assert_eq!(<&str>::from(Kind::PatchPanel), "patch-panel");
        assert!(Kind::VARIANTS.contains(&"ups"));
    }
}
