//! Looking up the wiring's endpoints in the rack and checking its cables.

use std::collections::HashMap;

use miette::SourceSpan;

use super::cabling::{Cabling, DeviceEndpoints, End, Link, SocketId};
use super::endpoint::main_part;
use super::{CablePath, EndpointName, EndpointRef, LinkKind, PatchSide, Wiring, parse_endpoint};
use crate::catalog::{Catalog, Kind, Model, Part, part_names};
use crate::kdl_reader::Problem;
use crate::rack::Rack;

/// An endpoint found in the rack.
#[derive(Debug, Clone, Copy)]
struct Found {
    socket: SocketId,
    part: Part,
    /// For a patch-panel port, the side the cable arriving plugs into and the side of the
    /// cable leaving; at an end of a path, both are the side written.
    sides: Option<(PatchSide, PatchSide)>,
}

/// Where an endpoint is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// First or last in a path.
    End,
    /// Between two others in a path.
    Middle,
    /// On its own, as in `rackctl trace`: a patch-panel port may leave out its side.
    Alone,
}

/// The rack's devices and their models, for looking up endpoints.
struct Devices<'a> {
    rack: &'a Rack,
    models: Vec<Option<&'a Model>>,
    ids: HashMap<&'a str, usize>,
}

impl<'a> Devices<'a> {
    fn new(rack: &'a Rack, catalog: &'a Catalog) -> Self {
        Self {
            rack,
            models: rack.devices.iter().map(|device| catalog.model(&device.model).ok()).collect(),
            ids: rack
                .devices
                .iter()
                .enumerate()
                .map(|(index, device)| (device.id.as_str(), index))
                .collect(),
        }
    }
}

impl Wiring {
    /// Looks up every endpoint in `rack`, whose models come from `catalog`, and checks the
    /// cables: the endpoints they connect, and that each endpoint, or each side of a patch
    /// port, takes one cable.
    ///
    /// # Errors
    ///
    /// Returns every problem found, located in the wiring file.
    pub fn resolve(&self, rack: &Rack, catalog: &Catalog) -> Result<Cabling, Vec<Problem>> {
        let devices = Devices::new(rack, catalog);
        let endpoints: Vec<DeviceEndpoints> = devices
            .models
            .iter()
            .map(|model| DeviceEndpoints {
                counts: Part::ENDPOINTS.map(|part| model.map_or(0, |m| m.components.count(part))),
                patch_panel: model.is_some_and(|model| model.kind == Kind::PatchPanel),
            })
            .collect();
        let mut cabling = Cabling::new(&endpoints);
        // Where each socket, or each side of a patch port, is first used, by `End::place`.
        let mut first_uses: Vec<Option<SourceSpan>> = vec![None; cabling.places()];
        let mut problems = Vec::new();
        for path in &self.paths {
            let before = problems.len();
            let last = path.endpoints.len().saturating_sub(1);
            let mut found = Vec::new();
            for (at, end) in path.endpoints.iter().enumerate() {
                let place = if at == 0 || at == last { Place::End } else { Place::Middle };
                match devices.find(end, place, &cabling) {
                    Ok(end) => found.push(end),
                    Err(problem) => problems.push(problem),
                }
            }
            if problems.len() == before {
                check_parts(path, &found, &mut problems);
            }
            if problems.len() > before {
                continue;
            }
            for (pair, written) in found.windows(2).zip(path.endpoints.windows(2)) {
                let ends = [
                    End { socket: pair[0].socket, side: pair[0].sides.map(|(_, exit)| exit) },
                    End { socket: pair[1].socket, side: pair[1].sides.map(|(entry, _)| entry) },
                ];
                let mut free = true;
                for (end, written) in ends.into_iter().zip(written) {
                    let first_use = &mut first_uses[end.place()];
                    match *first_use {
                        Some(first) => {
                            problems.push(already_connected(end, written, first));
                            free = false;
                        }
                        None => *first_use = Some(written.span),
                    }
                }
                if free {
                    cabling.connect(Link { kind: path.kind, ends });
                }
            }
        }
        if problems.is_empty() { Ok(cabling) } else { Err(problems) }
    }
}

impl Cabling {
    /// Looks up an endpoint written as in the wiring, such as `srv01:psu1`, in `rack`, whose
    /// models come from `catalog`. A patch-panel port may leave out its side.
    ///
    /// # Errors
    ///
    /// Returns why `text` is not an endpoint of the rack.
    pub fn find(&self, text: &str, rack: &Rack, catalog: &Catalog) -> Result<SocketId, Problem> {
        let span = SourceSpan::from(0..text.len());
        let (device, name) = parse_endpoint(text).map_err(|message| Problem::new(message, span))?;
        let end = EndpointRef { device: device.to_owned(), name, span };
        Ok(Devices::new(rack, catalog).find(&end, Place::Alone, self)?.socket)
    }
}

impl Devices<'_> {
    /// Looks up an endpoint written at `place`.
    fn find(&self, end: &EndpointRef, place: Place, cabling: &Cabling) -> Result<Found, Problem> {
        // The endpoint as written, for messages only.
        let written = || format!("{}:{}", end.device, end.name);
        let problem = |message: String| Problem::new(message, end.span);
        let Some(&device) = self.ids.get(end.device.as_str()) else {
            return Err(problem(format!("unknown device `{}`", end.device))
                .with_label("not in the rack")
                // In file order, so that a tie is always settled the same way.
                .with_suggestion(&end.device, self.rack.devices.iter().map(|d| d.id.as_str())));
        };
        let id = &self.rack.devices[device].id;
        let Some(model) = self.models[device] else {
            return Err(problem(format!("the model of `{id}` has problems"))
                .with_label("see the problems reported for its file"));
        };
        let kind: &str = model.kind.into();
        if place == Place::Middle && model.kind != Kind::PatchPanel {
            return Err(problem("only a patch-panel port can sit in the middle of a path".into())
                .with_label(format!("`{id}` is a {kind}")));
        }
        let wanted = wanted(&end.name, model.kind, place).map_err(|mistake| {
            let problem = problem(mistake.message(&written(), id, kind));
            if matches!(mistake, Mistake::NoMainList) {
                problem.with_help(format!("name the endpoint, such as `{id}:nic1` or `{id}:psu1`"))
            } else {
                problem
            }
        })?;

        let Wanted { part, group, number, sides } = wanted;
        let index = match number {
            Some(number) => model.endpoint_index(part, group, number),
            None if group.is_none() && model.components.count(part) == 1 => Some(0),
            None => None,
        };
        let socket = index.and_then(|index| cabling.socket(device, part, index));
        let Some(socket) = socket else {
            let written = written();
            let (noun, _) = part_names(part);
            return Err(problem(match (describe_endpoints(model, part), number) {
                (Some(endpoints), None) => {
                    format!("`{written}` needs a number: `{id}` has {endpoints}")
                }
                (Some(endpoints), Some(_)) => {
                    format!("`{written}` does not exist: `{id}` has {endpoints}")
                }
                (None, _) => {
                    format!("`{written}` does not exist: the model of `{id}` has no {noun}s")
                }
            }));
        };
        Ok(Found { socket, part, sides })
    }
}

/// The endpoint a name asks for, before it is looked up on the model.
struct Wanted<'a> {
    part: Part,
    group: Option<&'a str>,
    number: Option<u16>,
    sides: Option<(PatchSide, PatchSide)>,
}

/// Why a name does not fit the kind of its device.
enum Mistake {
    /// A patch-panel port without its side.
    NoSide,
    /// A bare number on a device without a main list.
    NoMainList,
    /// One side in the middle of a path.
    OneSide,
    /// A word that is not a side, on a patch panel.
    NotASide(String),
    /// A side on a device other than a patch panel.
    NotAPatchPanel,
}

impl Mistake {
    /// Describes the mistake in the endpoint `written` of the device `id` of `kind`.
    fn message(&self, written: &str, id: &str, kind: &str) -> String {
        match self {
            Self::NoSide => format!(
                "`{written}` needs a side: a patch-panel port is written such as `{id}:b14` or \
                 `{id}:f14`"
            ),
            Self::NoMainList => {
                format!("`{written}` needs an endpoint name: a {kind} has no numbered main list")
            }
            Self::OneSide => format!(
                "in the middle of a path, a patch port gives both sides, such as `{id}:b-f14`"
            ),
            Self::NotASide(word) => {
                format!("a patch side is `b`, `back`, `f` or `front`, found `{word}`")
            }
            Self::NotAPatchPanel => {
                format!("only patch-panel ports have sides: `{id}` is a {kind}")
            }
        }
    }
}

/// Reads what `name`, written at `place`, asks for on a device of `kind`.
fn wanted(name: &EndpointName, kind: Kind, place: Place) -> Result<Wanted<'_>, Mistake> {
    let wanted = |part, group, number, sides| Wanted { part, group, number, sides };
    if kind == Kind::PatchPanel {
        return match name {
            EndpointName::Through { from, number } => {
                Ok(wanted(Part::Port, None, Some(*number), Some((*from, from.opposite()))))
            }
            EndpointName::Main(number) if place == Place::Alone => {
                Ok(wanted(Part::Port, None, Some(*number), None))
            }
            EndpointName::Main(_) => Err(Mistake::NoSide),
            EndpointName::Named { word, number } => match PatchSide::parse(word) {
                Some(_) if place == Place::Middle => Err(Mistake::OneSide),
                Some(side) => Ok(wanted(Part::Port, None, *number, Some((side, side)))),
                None => match endpoint_part(word) {
                    Some(Part::Port) if place == Place::Alone => {
                        Ok(wanted(Part::Port, None, *number, None))
                    }
                    Some(Part::Port) => Err(Mistake::NoSide),
                    Some(part) => Ok(wanted(part, None, *number, None)),
                    None => Err(Mistake::NotASide(word.clone())),
                },
            },
        };
    }
    match name {
        EndpointName::Main(number) => {
            let part = main_part(kind).ok_or(Mistake::NoMainList)?;
            Ok(wanted(part, None, Some(*number), None))
        }
        EndpointName::Named { word, .. } if PatchSide::parse(word).is_some() => {
            Err(Mistake::NotAPatchPanel)
        }
        EndpointName::Named { word, number } => {
            let part = endpoint_part(word);
            let group = part.is_none().then_some(word.as_str());
            Ok(wanted(part.unwrap_or(Part::Port), group, *number, None))
        }
        EndpointName::Through { .. } => Err(Mistake::NotAPatchPanel),
    }
}

/// Reports endpoints that do not fit the link: power runs from an outlet into a PSU, and
/// data links connect NICs, management ports and ports.
fn check_parts(path: &CablePath, found: &[Found], problems: &mut Vec<Problem>) {
    let wrong = |end: &EndpointRef, found: &Found, message: String| {
        let (noun, _) = part_names(found.part);
        Problem::new(message, end.span).with_label(format!("this is {}", with_article(noun)))
    };
    let pairs = path.endpoints.iter().zip(found);
    match path.kind {
        LinkKind::Power => {
            for (index, (end, found)) in pairs.enumerate() {
                let (part, message) = if index == 0 {
                    (Part::Outlet, "a `power` link starts at an outlet")
                } else {
                    (Part::Psu, "a `power` link ends at a PSU")
                };
                if found.part != part {
                    problems.push(wrong(end, found, message.to_owned()));
                }
            }
        }
        LinkKind::Net | LinkKind::Mgmt => {
            let kind: &str = path.kind.into();
            for (end, found) in pairs {
                if !matches!(found.part, Part::Nic | Part::Mgmt | Part::Port) {
                    let message =
                        format!("a `{kind}` link connects NICs, management ports and ports");
                    problems.push(wrong(end, found, message));
                }
            }
        }
    }
}

/// Reports an endpoint, or a side of a patch port, that already takes a cable.
fn already_connected(end: End, written: &EndpointRef, first: SourceSpan) -> Problem {
    let side = end.side.map_or_else(String::new, |side| format!(" at the {}", side.name()));
    let message = format!("`{}:{}` is already connected{side}", written.device, written.name);
    Problem::new(message, written.span)
        .with_label("connected again here")
        .with_label_at(first, "first connected here")
}

/// Returns the endpoint part `word` names, such as `psu`.
fn endpoint_part(word: &str) -> Option<Part> {
    Part::ENDPOINTS.into_iter().find(|&part| <&str>::from(part) == word)
}

/// Describes the endpoints of `part` on `model`, such as `ports 1-48, XG1-XG4`, or `None`
/// when it has none.
fn describe_endpoints(model: &Model, part: Part) -> Option<String> {
    let (noun, _) = part_names(part);
    let mut total = 0;
    let mut runs = Vec::new();
    for run in model.endpoint_runs(part) {
        let group = run.group.unwrap_or("");
        let last = run.first + run.count.saturating_sub(1);
        total += usize::from(run.count);
        runs.push(if run.count == 1 {
            format!("{group}{last}")
        } else {
            format!("{group}{}-{group}{last}", run.first)
        });
    }
    let runs = runs.join(", ");
    match total {
        0 => None,
        1 => Some(format!("{noun} {runs}")),
        _ => Some(format!("{noun}s {runs}")),
    }
}

/// Puts `a` or `an` before a noun.
fn with_article(noun: &str) -> String {
    let article = if noun.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{article} {noun}")
}

#[cfg(test)]
pub(super) mod tests {
    use indoc::indoc;
    use miette::Diagnostic;

    use super::*;
    use crate::testing::pointed;

    const RACK: &str = indoc! {r#"
        rack "lab" units=10 {
          device "sw" model="cisco/sg350x-48p" u=10
          device "patch" model="generic/patchpanel-24" u=9
          device "router" model="ubiquiti/er6p" u=8
          device "srv01" model="dell/r630-sff8" u=7
          device "srv02" model="dell/r630-sff8" u=6
          device "pdu" model="apc/ap7952" mount="right"
        }"#};

    /// Resolves the wiring statements `paths` against [`RACK`] and the built-in catalog.
    pub(in crate::wiring) fn resolve(paths: &str) -> (String, Rack, Result<Cabling, Vec<Problem>>) {
        let catalog = Catalog::builtin();
        let rack = Rack::parse(RACK).expect("valid rack");
        assert!(rack.check(&catalog).is_empty());
        let text = format!("wiring {{\n{paths}\n}}");
        let result = Wiring::parse(&text).expect("valid wiring").resolve(&rack, &catalog);
        (text, rack, result)
    }

    /// Asserts the problems of `paths`, each as its message and the text it points at.
    #[track_caller]
    fn assert_reported(paths: &str, expected: &[(&str, &str)]) {
        let (text, _, result) = resolve(paths);
        let problems = result.expect_err("problems");
        assert_eq!(pointed(&text, &problems), expected);
    }

    #[test]
    fn connects_the_endpoints_along_each_path() {
        let (_, rack, result) = resolve(indoc! {"
            power pdu:1 srv01:psu1
            power pdu:outlet2 srv01:psu2
            mgmt srv01:mgmt patch:b-f1 sw:1
            net srv01:nic1 patch:back2
            net router:0 sw:XG1
            net sw:48 patch:f2"});
        let cabling = result.expect("valid wiring");
        let describe = |end: End| {
            let endpoint = cabling.endpoint(end.socket);
            let part: &str = endpoint.part.into();
            let side = end.side.map_or(String::new(), |side| format!(" {}", side.name()));
            format!("{}:{part}{}{side}", rack.devices[endpoint.device].id, endpoint.index)
        };
        let links: Vec<String> = cabling
            .links()
            .iter()
            .map(|link| {
                let kind: &str = link.kind.into();
                format!("{kind} {} - {}", describe(link.ends[0]), describe(link.ends[1]))
            })
            .collect();
        assert_eq!(
            links,
            [
                "power pdu:outlet0 - srv01:psu0",
                "power pdu:outlet1 - srv01:psu1",
                "mgmt srv01:mgmt0 - patch:port0 back",
                "mgmt patch:port0 front - sw:port0",
                "net srv01:nic0 - patch:port1 back",
                "net router:port0 - sw:port48",
                "net sw:port47 - patch:port1 front",
            ]
        );
        let front = End {
            socket: cabling.socket(1, Part::Port, 0).expect("port 1"),
            side: Some(PatchSide::Front),
        };
        assert_eq!(cabling.link_at(front), Some(&cabling.links()[3]));
    }

    #[test]
    fn suggests_the_first_of_equally_close_devices() {
        // `srv0` is one edit from both `srv01` and `srv02`; the one written first wins.
        for _ in 0..20 {
            let (_, _, result) = resolve("net srv0:nic1 sw:1");
            let problems = result.expect_err("problems");
            assert_eq!(problems[0].help(), Some("did you mean `srv01`?"));
        }
    }

    #[test]
    fn reports_endpoints_the_rack_does_not_have() {
        assert_reported(
            indoc! {"
                power pdu:25 srv01:psu1
                power pdu:2 srv01:psu3
                net ghost:nic1 sw:1
                net srv01:nic sw:2
                net srv01:3 sw:3
                net router:6 sw:4
                net sw:XG5 router:1
                mgmt srv01:outlet1 sw:9
                mgmt srv02:mgmt2 sw:10"},
            &[
                ("`pdu:25` does not exist: `pdu` has outlets 1-24", "pdu:25"),
                ("`srv01:psu3` does not exist: `srv01` has PSUs 1-2", "srv01:psu3"),
                ("unknown device `ghost`", "ghost:nic1"),
                ("`srv01:nic` needs a number: `srv01` has NICs 1-4", "srv01:nic"),
                ("`srv01:3` needs an endpoint name: a server has no numbered main list", "srv01:3"),
                ("`router:6` does not exist: `router` has ports 0-5", "router:6"),
                ("`sw:XG5` does not exist: `sw` has ports 1-48, XG1-XG4", "sw:XG5"),
                (
                    "`srv01:outlet1` does not exist: the model of `srv01` has no outlets",
                    "srv01:outlet1",
                ),
                ("`srv02:mgmt2` does not exist: `srv02` has management port 1", "srv02:mgmt2"),
            ],
        );
    }

    #[test]
    fn reports_patch_ports_without_their_sides() {
        assert_reported(
            indoc! {"
                net srv02:nic1 patch:3
                net srv02:nic2 patch:b4 sw:5
                net srv02:nic3 srv01:nic4 sw:6
                net sw:b7 srv02:nic4
                net patch:x1 sw:8"},
            &[
                (
                    "`patch:3` needs a side: a patch-panel port is written such as `patch:b14` \
                     or `patch:f14`",
                    "patch:3",
                ),
                (
                    "in the middle of a path, a patch port gives both sides, such as \
                     `patch:b-f14`",
                    "patch:b4",
                ),
                ("only a patch-panel port can sit in the middle of a path", "srv01:nic4"),
                ("only patch-panel ports have sides: `sw` is a switch", "sw:b7"),
                ("a patch side is `b`, `back`, `f` or `front`, found `x`", "patch:x1"),
            ],
        );
    }

    #[test]
    fn reports_links_between_the_wrong_parts() {
        assert_reported(
            indoc! {"
                power srv01:psu1 pdu:1
                power pdu:2 srv01:nic1
                net pdu:nic1 srv01:psu2"},
            &[
                ("a `power` link starts at an outlet", "srv01:psu1"),
                ("a `power` link ends at a PSU", "pdu:1"),
                ("a `power` link ends at a PSU", "srv01:nic1"),
                ("a `net` link connects NICs, management ports and ports", "srv01:psu2"),
            ],
        );
    }

    #[test]
    fn reports_endpoints_used_twice() {
        assert_reported(
            indoc! {"
                power pdu:1 srv01:psu1
                power pdu:1 srv02:psu1
                net srv01:nic1 patch:b1
                net srv02:nic1 patch:back1
                net sw:1 patch:f1
                net sw:2 sw:2"},
            &[
                ("`pdu:1` is already connected", "pdu:1"),
                ("`patch:back1` is already connected at the back", "patch:back1"),
                ("`sw:2` is already connected", "sw:2"),
            ],
        );

        // The problem also points at the first use.
        let (text, _, result) = resolve("power pdu:1 srv01:psu1\npower pdu:1 srv02:psu1");
        let problems = result.expect_err("problems");
        let labels: Vec<(String, usize)> = problems[0]
            .labels()
            .expect("labels")
            .map(|label| (label.label().unwrap_or_default().to_owned(), label.offset()))
            .collect();
        let first = text.find("pdu:1").expect("first use");
        let again = text.rfind("pdu:1").expect("second use");
        assert_eq!(
            labels,
            [
                ("connected again here".to_owned(), again),
                ("first connected here".to_owned(), first)
            ]
        );
    }
}
