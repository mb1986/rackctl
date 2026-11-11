//! Looking up the wiring's endpoints in the layout and checking its cables.

use miette::SourceSpan;

use super::cabling::{Cabling, DeviceEndpoints, End, Link, SocketId};
use super::endpoint::main_part;
use super::{CablePath, EndpointName, EndpointRef, LinkKind, PatchSide, Wiring, parse_endpoint};
use crate::catalog::{Catalog, Kind, Model, Part, part_names};
use crate::kdl_reader::{FileId, Place, Problem};
use crate::rack::{DeviceRef, Layout};

/// An endpoint found in the layout.
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
enum Position {
    /// First or last in a path.
    End,
    /// Between two others in a path.
    Middle,
    /// On its own, as in `rackctl trace`: a patch-panel port may leave out its side.
    Alone,
}

/// The layout's devices and their models, for looking up endpoints.
struct Devices<'a> {
    layout: &'a Layout,
    /// The model of each device, by [`DeviceId`].
    models: Vec<Option<&'a Model>>,
}

impl<'a> Devices<'a> {
    fn new(layout: &'a Layout, catalog: &'a Catalog) -> Self {
        let models = layout.devices().map(|device| catalog.model(&device.model).ok()).collect();
        Self { layout, models }
    }
}

impl Wiring {
    /// Resolves this wiring alone, as [`resolve`] does for every wiring of a configuration.
    ///
    /// # Errors
    ///
    /// Returns every problem found, located in the wiring's file.
    pub fn resolve(&self, layout: &Layout, catalog: &Catalog) -> Result<Cabling, Vec<Problem>> {
        resolve(std::slice::from_ref(self), layout, catalog).map_err(|problems| {
            let settle =
                |(file, problem): (FileId, Problem)| problem.settle(file, |_| String::new());
            problems.into_iter().map(settle).collect()
        })
    }
}

/// Looks up every endpoint of `wirings` in `layout`, whose models come from `catalog`, and
/// checks the cables.
///
/// A cable connects endpoints that fit its type, and each endpoint, or each side of a patch
/// port, takes one cable. A device written by its name alone is looked up in the rack its
/// wiring names, if any.
///
/// # Errors
///
/// Returns every problem found, each with the file it is located in.
pub fn resolve(
    wirings: &[Wiring],
    layout: &Layout,
    catalog: &Catalog,
) -> Result<Cabling, Vec<(FileId, Problem)>> {
    let devices = Devices::new(layout, catalog);
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
    let mut first_uses: Vec<Option<Place>> = vec![None; cabling.places()];
    let mut all = Vec::new();
    for wiring in wirings {
        let mut problems = Vec::new();
        wiring.connect(&devices, &mut cabling, &mut first_uses, &mut problems);
        all.extend(problems.into_iter().map(|problem| (wiring.file, problem)));
    }
    if all.is_empty() { Ok(cabling) } else { Err(all) }
}

impl Wiring {
    /// Adds the cables of this wiring to `cabling`. `first_uses` tells where each socket, or
    /// each side of a patch port, is first used.
    fn connect(
        &self,
        devices: &Devices,
        cabling: &mut Cabling,
        first_uses: &mut [Option<Place>],
        problems: &mut Vec<Problem>,
    ) {
        let rack = match &self.rack {
            Some(rack) => match devices.layout.find_rack(&rack.value, rack.span) {
                Ok(_) => Some(rack.value.as_str()),
                Err(problem) => {
                    problems.push(problem);
                    return;
                }
            },
            None => None,
        };
        for path in &self.paths {
            let before = problems.len();
            let last = path.endpoints.len().saturating_sub(1);
            let mut found = Vec::new();
            for (at, end) in path.endpoints.iter().enumerate() {
                let place = if at == 0 || at == last { Position::End } else { Position::Middle };
                match devices.find(end, place, cabling, rack) {
                    Ok(end) => found.push(end),
                    Err(problem) => problems.push(problem),
                }
            }
            if problems.len() == before {
                check_parts(path, &found, problems);
            }
            if problems.len() > before {
                continue;
            }
            for (pair, written) in found.windows(2).zip(path.endpoints.windows(2)) {
                if pair[0].socket == pair[1].socket {
                    problems.push(prb::connected_to_itself(pair[1].part, &written[1], &written[0]));
                    continue;
                }
                let ends = [
                    End { socket: pair[0].socket, side: pair[0].sides.map(|(_, exit)| exit) },
                    End { socket: pair[1].socket, side: pair[1].sides.map(|(entry, _)| entry) },
                ];
                let mut free = true;
                for (end, written) in ends.into_iter().zip(written) {
                    let first_use = &mut first_uses[end.place()];
                    match *first_use {
                        Some(first) => {
                            problems.push(prb::already_connected(end, written, first));
                            free = false;
                        }
                        None => *first_use = Some(Place { file: self.file, span: written.span }),
                    }
                }
                if free {
                    cabling.connect(Link { kind: path.kind, ends });
                }
            }
        }
    }
}

impl Cabling {
    /// Looks up an endpoint written as in the wiring, such as `srv01:psu1`, in `layout`,
    /// whose models come from `catalog`. A patch-panel port may leave out its side.
    ///
    /// # Errors
    ///
    /// Returns why `text` is not an endpoint of the layout.
    pub fn find(
        &self,
        text: &str,
        layout: &Layout,
        catalog: &Catalog,
    ) -> Result<SocketId, Problem> {
        let span = SourceSpan::from(0..text.len());
        let (device, name) =
            parse_endpoint(text).map_err(|message| prb::not_an_endpoint(message, span))?;
        let end = EndpointRef { device: device.to_owned(), name, span };
        Ok(Devices::new(layout, catalog).find(&end, Position::Alone, self, None)?.socket)
    }
}

impl Devices<'_> {
    /// Looks up an endpoint written at `place`. A device written by its name alone is the one
    /// of `rack`, when a rack is given.
    fn find(
        &self,
        end: &EndpointRef,
        place: Position,
        cabling: &Cabling,
        rack: Option<&str>,
    ) -> Result<Found, Problem> {
        let device = self.layout.find_device(DeviceRef::parse(&end.device), rack, end.span)?;
        let id = &self.layout.device_name(device);
        let Some(model) = self.models[device.0] else {
            return Err(prb::invalid_model(end, id));
        };
        if place == Position::Middle && model.kind != Kind::PatchPanel {
            return Err(prb::not_a_patch_port_in_the_middle(end, id, model.kind));
        }
        let Wanted { part, group, number, sides } = wanted(&end.name, model.kind, place)
            .map_err(|mistake| prb::mistake(&mistake, end, id, model.kind))?;
        let index = match number {
            Some(number) => model.endpoint_index(part, group, number),
            None if group.is_none() && model.components.count(part) == 1 => Some(0),
            None => None,
        };
        let socket = index.and_then(|index| cabling.socket(device, part, index));
        let Some(socket) = socket else {
            let endpoints = describe_endpoints(model, part);
            return Err(prb::no_such_endpoint(end, id, part, endpoints, number.is_some()));
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
    /// A patch-panel port without both sides in the middle of a path.
    NotBothSides,
    /// A word that is not a side, on a patch panel.
    NotASide(String),
    /// A side on a device other than a patch panel.
    NotAPatchPanel,
    /// A part name with capitals, such as `PSU`, and the name written correctly.
    Capitalised { word: String, fixed: String },
}

/// Returns the mistake of a part name written with capitals, such as `PSU1`.
fn capitalised(word: &str, number: Option<u16>) -> Option<Mistake> {
    let part: &str = endpoint_part(&word.to_ascii_lowercase())?.into();
    let fixed = number.map_or_else(|| part.to_owned(), |number| format!("{part}{number}"));
    Some(Mistake::Capitalised { word: word.to_owned(), fixed })
}

/// Reads what `name`, written at `place`, asks for on a device of `kind`.
fn wanted(name: &EndpointName, kind: Kind, place: Position) -> Result<Wanted<'_>, Mistake> {
    let wanted = |part, group, number, sides| Wanted { part, group, number, sides };
    if kind == Kind::PatchPanel {
        return match name {
            EndpointName::Through { from, number } => {
                Ok(wanted(Part::Port, None, Some(*number), Some((*from, from.opposite()))))
            }
            EndpointName::Main(number) if place == Position::Alone => {
                Ok(wanted(Part::Port, None, Some(*number), None))
            }
            EndpointName::Main(_) if place == Position::Middle => Err(Mistake::NotBothSides),
            EndpointName::Main(_) => Err(Mistake::NoSide),
            EndpointName::Named { word, number } => match PatchSide::parse(word) {
                Some(_) if place == Position::Middle => Err(Mistake::NotBothSides),
                Some(side) => Ok(wanted(Part::Port, None, *number, Some((side, side)))),
                None => match endpoint_part(word) {
                    Some(Part::Port) if place == Position::Alone => {
                        Ok(wanted(Part::Port, None, *number, None))
                    }
                    Some(Part::Port) if place == Position::Middle => Err(Mistake::NotBothSides),
                    Some(Part::Port) => Err(Mistake::NoSide),
                    Some(part) => Ok(wanted(part, None, *number, None)),
                    None => Err(capitalised(word, *number)
                        .unwrap_or_else(|| Mistake::NotASide(word.clone()))),
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
            if part.is_none()
                && let Some(mistake) = capitalised(word, *number)
            {
                return Err(mistake);
            }
            let group = part.is_none().then_some(word.as_str());
            Ok(wanted(part.unwrap_or(Part::Port), group, *number, None))
        }
        EndpointName::Through { .. } => Err(Mistake::NotAPatchPanel),
    }
}

/// Reports endpoints that do not fit the link: power runs from an outlet into a PSU, and
/// data links connect NICs, management ports and ports.
fn check_parts(path: &CablePath, found: &[Found], problems: &mut Vec<Problem>) {
    let pairs = path.endpoints.iter().zip(found);
    match path.kind {
        LinkKind::Power => {
            for (index, (end, found)) in pairs.enumerate() {
                if index == 0 && found.part != Part::Outlet {
                    problems.push(prb::power_not_from_an_outlet(end, found.part));
                } else if index > 0 && found.part != Part::Psu {
                    problems.push(prb::power_not_into_a_psu(end, found.part));
                }
            }
        }
        LinkKind::Net | LinkKind::Mgmt => {
            for (end, found) in pairs {
                if !matches!(found.part, Part::Nic | Part::Mgmt | Part::Port) {
                    problems.push(prb::not_a_data_endpoint(path.kind, end, found.part));
                }
            }
        }
    }
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

/// The problems reported for the wiring's endpoints and cables.
mod prb {
    use miette::SourceSpan;

    use super::{End, EndpointRef, LinkKind, Mistake};
    use crate::catalog::{Kind, Part, part_names};
    use crate::kdl_reader::{Place, Problem};

    /// `message` says why the text at `span` is not an endpoint.
    pub(super) fn not_an_endpoint(message: String, span: SourceSpan) -> Problem {
        Problem::new(message, span)
    }

    /// `id` names the device of `end`.
    pub(super) fn invalid_model(end: &EndpointRef, id: &str) -> Problem {
        Problem::new(format!("the model of `{id}` has problems"), end.span)
            .with_label("see the problems reported for its file")
    }

    /// `id` names the device of `end`, which is of `kind`.
    pub(super) fn not_a_patch_port_in_the_middle(
        end: &EndpointRef,
        id: &str,
        kind: Kind,
    ) -> Problem {
        let kind: &str = kind.into();
        Problem::new("only a patch-panel port can sit in the middle of a path", end.span)
            .with_label(format!("`{id}` is a {kind}"))
    }

    /// `id` names the device of `end`, which is of `kind`.
    pub(super) fn mistake(mistake: &Mistake, end: &EndpointRef, id: &str, kind: Kind) -> Problem {
        let written = written(end);
        let kind: &str = kind.into();
        let message = match mistake {
            Mistake::NoSide => format!(
                "`{written}` needs a side: a patch-panel port is written such as `{id}:b14` or \
                 `{id}:f14`"
            ),
            Mistake::NoMainList => {
                format!("`{written}` needs an endpoint name: a {kind} has no numbered main list")
            }
            Mistake::NotBothSides => format!(
                "in the middle of a path, a patch port gives both sides, such as `{id}:b-f14`"
            ),
            Mistake::NotASide(word) => {
                format!("a patch side is `b`, `back`, `f` or `front`, found `{word}`")
            }
            Mistake::NotAPatchPanel => {
                format!("only patch-panel ports have sides: `{id}` is a {kind}")
            }
            Mistake::Capitalised { word, fixed } => format!(
                "`{written}` names the part `{word}`: part names are lowercase, such as \
                 `{id}:{fixed}`"
            ),
        };
        let problem = Problem::new(message, end.span);
        if matches!(mistake, Mistake::NoMainList) {
            problem.with_help(format!("name the endpoint, such as `{id}:nic1` or `{id}:psu1`"))
        } else {
            problem
        }
    }

    /// `id` names the device of `end`. `endpoints` describes its endpoints of `part`, if it
    /// has any, and `numbered` tells whether `end` gives a number.
    pub(super) fn no_such_endpoint(
        end: &EndpointRef,
        id: &str,
        part: Part,
        endpoints: Option<String>,
        numbered: bool,
    ) -> Problem {
        let written = written(end);
        let message = match endpoints {
            Some(endpoints) if numbered => {
                format!("`{written}` does not exist: `{id}` has {endpoints}")
            }
            Some(endpoints) => format!("`{written}` needs a number: `{id}` has {endpoints}"),
            None => {
                let (noun, _) = part_names(part);
                format!("`{written}` does not exist: the model of `{id}` has no {noun}s")
            }
        };
        Problem::new(message, end.span)
    }

    /// `end` is of `part`.
    pub(super) fn power_not_from_an_outlet(end: &EndpointRef, part: Part) -> Problem {
        wrong_part("a `power` link starts at an outlet", end, part)
    }

    /// `end` is of `part`.
    pub(super) fn power_not_into_a_psu(end: &EndpointRef, part: Part) -> Problem {
        wrong_part("a `power` link ends at a PSU", end, part)
    }

    /// `end` is of `part`, on a link of `kind`.
    pub(super) fn not_a_data_endpoint(kind: LinkKind, end: &EndpointRef, part: Part) -> Problem {
        let kind: &str = kind.into();
        let message = format!("a `{kind}` link connects NICs, management ports and ports");
        wrong_part(message, end, part)
    }

    /// The two ends of a cable, `end` and `other`, are the same endpoint of `part`.
    pub(super) fn connected_to_itself(
        part: Part,
        end: &EndpointRef,
        other: &EndpointRef,
    ) -> Problem {
        let (noun, _) = part_names(part);
        let message = format!("a cable cannot connect {} to itself", with_article(noun));
        Problem::new(message, end.span).with_label("the same").with_label_at(other.span, "as this")
    }

    /// `written` is an endpoint, or a side of a patch port, first used at `first`.
    pub(super) fn already_connected(end: End, written: &EndpointRef, first: Place) -> Problem {
        let side = end.side.map_or_else(String::new, |side| format!(" at the {}", side.name()));
        let message = format!("`{}` is already connected{side}", self::written(written));
        Problem::new(message, written.span)
            .with_label("connected again here")
            .with_place(first, "first connected")
    }

    fn wrong_part(message: impl Into<String>, end: &EndpointRef, part: Part) -> Problem {
        let (noun, _) = part_names(part);
        Problem::new(message, end.span).with_label(format!("this is {}", with_article(noun)))
    }

    /// The endpoint as written.
    fn written(end: &EndpointRef) -> String {
        format!("{}:{}", end.device, end.name)
    }

    /// Puts `a` or `an` before a noun.
    fn with_article(noun: &str) -> String {
        let article = if noun.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
        format!("{article} {noun}")
    }
}

#[cfg(test)]
pub(super) mod tests {
    use indoc::indoc;
    use miette::Diagnostic;

    use super::*;
    use crate::rack::{DeviceId, Rack};
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
    pub(in crate::wiring) fn resolve(
        paths: &str,
    ) -> (String, Layout, Result<Cabling, Vec<Problem>>) {
        let catalog = Catalog::builtin();
        let rack = Rack::parse(RACK).expect("valid rack");
        assert!(rack.check(&catalog).is_empty());
        let layout = Layout::new(vec![rack]);
        let text = format!("wiring {{\n{paths}\n}}");
        let result = Wiring::parse(&text).expect("valid wiring").resolve(&layout, &catalog);
        (text, layout, result)
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
        let (_, layout, result) = resolve(indoc! {"
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
            let id = &layout.device(endpoint.device).1.id;
            format!("{id}:{part}{}{side}", endpoint.index)
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
            socket: cabling.socket(DeviceId(1), Part::Port, 0).expect("port 1"),
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
                mgmt srv02:mgmt2 sw:10
                power pdu:3 srv02:PSU1
                mgmt srv01:Mgmt sw:11"},
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
                (
                    "`srv02:PSU1` names the part `PSU`: part names are lowercase, such as \
                     `srv02:psu1`",
                    "srv02:PSU1",
                ),
                (
                    "`srv01:Mgmt` names the part `Mgmt`: part names are lowercase, such as \
                     `srv01:mgmt`",
                    "srv01:Mgmt",
                ),
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
                net patch:x1 sw:8
                net sw:11 patch:9 sw:12
                net sw:13 patch:port10 sw:14"},
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
                (
                    "in the middle of a path, a patch port gives both sides, such as \
                     `patch:b-f14`",
                    "patch:9",
                ),
                (
                    "in the middle of a path, a patch port gives both sides, such as \
                     `patch:b-f14`",
                    "patch:port10",
                ),
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
    fn reports_a_cable_from_an_endpoint_to_itself() {
        assert_reported(
            indoc! {"
                net patch:b5 patch:f5
                net sw:2 sw:2
                mgmt srv01:nic1 srv01:nic1"},
            &[
                ("a cable cannot connect a port to itself", "patch:f5"),
                ("a cable cannot connect a port to itself", "sw:2"),
                ("a cable cannot connect a NIC to itself", "srv01:nic1"),
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
                net sw:1 patch:f1"},
            &[
                ("`pdu:1` is already connected", "pdu:1"),
                ("`patch:back1` is already connected at the back", "patch:back1"),
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
