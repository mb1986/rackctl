//! Wiring: the cables between devices, as described in `wiring.kdl`.

mod endpoint;

use kdl::{KdlDocument, KdlNode};
use miette::SourceSpan;
use strum::{EnumString, IntoStaticStr, VariantNames};

pub use endpoint::{EndpointName, EndpointRef, PatchSide, parse_endpoint};

use crate::kdl_reader::{self, NodeReader, Problem, span_of};

/// The cables of `wiring.kdl` as written, before their endpoints are looked up in the rack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wiring {
    /// The statements in the order they appear in the file.
    pub paths: Vec<CablePath>,
}

/// One statement: cables of one type along a path, each pair of neighbouring endpoints being
/// one cable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CablePath {
    pub kind: LinkKind,
    pub endpoints: Vec<EndpointRef>,
    /// Location of the statement in the wiring file.
    pub span: SourceSpan,
}

/// What a cable carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "kebab-case")]
pub enum LinkKind {
    /// Power, from an outlet into a PSU.
    Power,
    /// Data between network ports.
    Net,
    /// Data to or from a management port.
    Mgmt,
}

impl Wiring {
    /// Parses a wiring file.
    ///
    /// # Errors
    ///
    /// Returns every problem found in the file.
    pub fn parse(text: &str) -> Result<Self, Vec<Problem>> {
        kdl_reader::parse_single(text, "wiring", |node, problems| Some(read_wiring(node, problems)))
    }
}

/// Reads the `wiring { ... }` node.
fn read_wiring(node: &KdlNode, problems: &mut Vec<Problem>) -> Wiring {
    let mut reader = NodeReader::new(node, problems);
    let block = reader.children();
    reader.finish();
    let paths = block
        .map(KdlDocument::nodes)
        .unwrap_or_default()
        .iter()
        .filter_map(|child| read_path(child, problems))
        .collect();
    Wiring { paths }
}

/// Reads a statement such as `net srv01:nic1 patch-32:b-f14 sg300:11`.
fn read_path(node: &KdlNode, problems: &mut Vec<Problem>) -> Option<CablePath> {
    let name = node.name().value();
    let Ok(kind) = name.parse::<LinkKind>() else {
        problems.push(
            Problem::new(format!("unknown link type `{name}`"), node.name().span())
                .with_suggestion(name, LinkKind::VARIANTS.iter().copied()),
        );
        return None;
    };
    let count = node.entries().iter().filter(|entry| entry.name().is_none()).count();
    let mut reader = NodeReader::new(node, problems);
    let texts: Vec<Option<String>> =
        (0..count).map(|index| reader.arg_str(index, "endpoint")).collect();
    reader.finish();

    let mut endpoints = Vec::new();
    for (index, text) in texts.into_iter().enumerate() {
        let span = span_of(node, index);
        match text.as_deref().map(parse_endpoint) {
            Some(Ok((device, name))) => {
                endpoints.push(EndpointRef { device: device.to_owned(), name, span });
            }
            Some(Err(message)) => problems.push(Problem::new(message, span)),
            None => {}
        }
    }
    if endpoints.len() < count {
        return None;
    }
    check_path(kind, node, &endpoints, problems);
    Some(CablePath { kind, endpoints, span: node.span() })
}

/// Reports a path with too few endpoints, a power cable through more than two, and a patch
/// port passed through at an end of the path.
fn check_path(
    kind: LinkKind,
    node: &KdlNode,
    endpoints: &[EndpointRef],
    problems: &mut Vec<Problem>,
) {
    let name: &str = kind.into();
    if endpoints.len() < 2 {
        problems.push(
            Problem::new(
                format!("a `{name}` link needs at least two endpoints"),
                node.name().span(),
            )
            .with_label("a cable has two ends"),
        );
        return;
    }
    if kind == LinkKind::Power
        && let Some(extra) = endpoints.get(2)
    {
        problems.push(
            Problem::new("a `power` link connects exactly two endpoints", extra.span)
                .with_help("write each power cable on its own line, from the outlet to the PSU"),
        );
    }
    for end in [&endpoints[0], &endpoints[endpoints.len() - 1]] {
        if let EndpointName::Through { number, .. } = end.name {
            problems.push(
                Problem::new("a path cannot pass through its first or last endpoint", end.span)
                    .with_help(format!("name the one side cabled here, such as `b{number}`")),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::testing::{covered, pointed};

    #[test]
    fn reads_paths_of_endpoints() {
        let text = indoc! {r#"
            wiring {
              power pdu:8 srv01:psu1
              mgmt "srv01:mgmt" patch-32:b-f15 sg300:10
            }"#};
        let wiring = Wiring::parse(text).expect("valid wiring");
        let path = |index: usize| {
            let path = &wiring.paths[index];
            let ends: Vec<(&str, &EndpointName)> =
                path.endpoints.iter().map(|end| (end.device.as_str(), &end.name)).collect();
            (path.kind, ends)
        };
        let named = |word: &str, number| EndpointName::Named { word: word.to_owned(), number };
        assert_eq!(
            path(0),
            (
                LinkKind::Power,
                vec![("pdu", &EndpointName::Main(8)), ("srv01", &named("psu", Some(1)))]
            )
        );
        let through = EndpointName::Through { from: PatchSide::Back, number: 15 };
        assert_eq!(
            path(1),
            (
                LinkKind::Mgmt,
                vec![
                    ("srv01", &named("mgmt", None)),
                    ("patch-32", &through),
                    ("sg300", &EndpointName::Main(10))
                ]
            )
        );
        assert_eq!(covered(text, wiring.paths[1].endpoints[1].span), "patch-32:b-f15");
        assert!(Wiring::parse("wiring {}").expect("empty wiring").paths.is_empty());
    }

    #[test]
    fn reports_each_problem_at_its_place() {
        let text = indoc! {r#"
            wiring {
              powr pdu:1 srv01:psu1
              net srv01:nic1
              power pdu:2 srv01:psu2 srv02:psu1
              net patch-32:b-f1 sg300:1
              net srv01 8 sg300:x-f2 sg300:3
              mgmt srv01:mgmt sg300:4 speed="1G"
            }"#};
        let problems = Wiring::parse(text).expect_err("problems");
        assert_eq!(
            pointed(text, &problems),
            [
                ("unknown link type `powr`", "powr"),
                ("a `net` link needs at least two endpoints", "net"),
                ("a `power` link connects exactly two endpoints", "srv02:psu1"),
                ("a path cannot pass through its first or last endpoint", "patch-32:b-f1"),
                ("`endpoint` must be a string, found the number 8", "8"),
                (
                    "`srv01` is not an endpoint: write `device:endpoint`, such as `pdu:8`, \
                     `srv01:psu1`, `sg350x:XG1`, `srv01:mgmt` or `patch-32:b14`",
                    "srv01"
                ),
                ("a patch side is `b`, `back`, `f` or `front`, found `x`", "sg300:x-f2"),
                ("unknown property `speed` on `mgmt`", r#"speed="1G""#),
            ]
        );
        assert_eq!(problems[0].help(), Some("did you mean `power`?"));
    }
}
