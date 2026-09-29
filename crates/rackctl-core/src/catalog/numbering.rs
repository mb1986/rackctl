//! Numbering: the numbers of a face's numbered elements, such as bays and ports.

use super::face::{Face, Faces};
use super::legend::{Direction, Legend, LegendEntry, Media, Part, PartKind};
use miette::SourceSpan;

use super::model::{Components, Model};
use crate::kdl_reader::{Problem, Spanned};
use crate::plural;

/// Where an element sits in the grid of its legend key: its row, and its column, which is
/// its place along the row.
#[derive(Debug, Clone, Copy)]
struct Position {
    /// The element's position in [`Face::elements`].
    index: usize,
    row: usize,
    column: usize,
}

/// The legend entries of one numbered part, which share the part's declared count, such as
/// `ports 52`. Each group of entries has its own numbers.
struct NumberedPart<'l> {
    part: Part,
    groups: Vec<Group<'l>>,
}

/// The elements a face shows of one legend entry, in numbering order, as positions in
/// [`Face::elements`], with the entry's group as a position in [`NumberedPart::groups`].
struct Shown<'l> {
    group: usize,
    entry: &'l LegendEntry,
    elements: Vec<usize>,
}

/// The legend entries of a part in one group, such as the ports of group `XG`.
struct Group<'l> {
    name: Option<&'l str>,
    entries: Vec<&'l LegendEntry>,
    /// The numbers the entries list with `numbers=`, sorted and without repeats.
    listed: Vec<u16>,
}

impl Group<'_> {
    /// Returns the entry without `numbers=`, which takes the numbers left.
    fn free(&self) -> Option<&LegendEntry> {
        self.entries.iter().copied().find(|entry| entry.numbering.numbers.is_none())
    }

    /// Names the group's elements in messages, such as `outlets` or ``ports in group `XG` ``,
    /// or `count` of them, such as `1 outlet`.
    fn describe(&self, part: Part, count: Option<usize>) -> String {
        let (name, _) = names(part);
        let elements = count.map_or_else(|| format!("{name}s"), |count| plural(count, name));
        match self.name {
            Some(group) => format!("{elements} in group `{group}`"),
            None => elements,
        }
    }
}

/// The elements that have one number, as positions in [`Face::elements`]: one element, or
/// for a combo port an RJ45 and an SFP element.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Slot {
    /// The RJ45 element of a network connector, or the element of any other part.
    pub element: Option<usize>,
    /// The SFP element of a network connector.
    pub sfp: Option<usize>,
}

impl Slot {
    /// Returns whether the slot is a combo port: an RJ45 and an SFP element.
    #[must_use]
    pub const fn is_combo(self) -> bool {
        self.element.is_some() && self.sfp.is_some()
    }
}

/// The numbers of one part on a face, such as its ports: the elements that have each number,
/// so that a status for port 17 finds its element directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbers {
    part: Part,
    /// The groups, in the order the legend first gives them, each a run of `slots`.
    segments: Vec<Segment>,
    slots: Vec<Slot>,
}

/// The slots of one group in a number table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    group: Option<String>,
    /// The number of the segment's first slot.
    base: u16,
    offset: usize,
    size: usize,
}

impl Numbers {
    /// Returns the part the table numbers.
    #[must_use]
    pub const fn part(&self) -> Part {
        self.part
    }

    /// Returns the lowest number of `group`, if the face shows it.
    #[must_use]
    pub fn base(&self, group: Option<&str>) -> Option<u16> {
        let segment = self.segments.iter().find(|segment| segment.group.as_deref() == group)?;
        Some(segment.base)
    }

    /// Returns the elements with `number` in `group`: port XG3 is `get(Some("XG"), 3)`.
    #[must_use]
    pub fn get(&self, group: Option<&str>, number: u16) -> Option<Slot> {
        let segment = self.segments.iter().find(|segment| segment.group.as_deref() == group)?;
        let at = usize::from(number.checked_sub(segment.base)?);
        (at < segment.size).then(|| self.slots[segment.offset + at])
    }

    /// Returns the numbers of each group, in table order.
    pub fn runs(&self) -> impl Iterator<Item = NumberRun<'_>> {
        self.segments.iter().map(|segment| NumberRun {
            group: segment.group.as_deref(),
            first: segment.base,
            count: u16::try_from(segment.size).unwrap_or(u16::MAX),
        })
    }
}

/// Consecutive numbers of one group, such as ports XG1 to XG4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberRun<'a> {
    pub group: Option<&'a str>,
    pub first: u16,
    pub count: u16,
}

impl NumberRun<'_> {
    /// Returns the place of `number` in the run, counted from 0.
    fn place(self, group: Option<&str>, number: u16) -> Option<u16> {
        let place = number.checked_sub(self.first)?;
        (self.group == group && place < self.count).then_some(place)
    }
}

impl Model {
    /// Returns the numbers of the endpoints of `part`, such as its ports: the number table of
    /// a face that shows the part, or else 1 to the declared count.
    pub fn endpoint_runs(&self, part: Part) -> impl Iterator<Item = NumberRun<'_>> {
        let table = self.faces.iter().find_map(|face| face.numbers(part));
        let count = self.components.count(part);
        let declared =
            (table.is_none() && count > 0).then_some(NumberRun { group: None, first: 1, count });
        table.into_iter().flat_map(Numbers::runs).chain(declared)
    }

    /// Returns the place of an endpoint among those of `part`, counted from 0 across its
    /// groups, or `None` if the model has no such endpoint.
    #[must_use]
    pub fn endpoint_index(&self, part: Part, group: Option<&str>, number: u16) -> Option<u16> {
        let mut offset = 0;
        for run in self.endpoint_runs(part) {
            if let Some(place) = run.place(group, number) {
                return Some(offset + place);
            }
            offset += run.count;
        }
        None
    }

    /// Returns the group and number of the endpoint of `part` at `index`.
    #[must_use]
    pub fn endpoint_name(&self, part: Part, index: u16) -> Option<(Option<&str>, u16)> {
        let mut rest = index;
        for run in self.endpoint_runs(part) {
            if rest < run.count {
                return Some((run.group, run.first + rest));
            }
            rest -= run.count;
        }
        None
    }
}

/// Numbers the elements of every face, and reports numberings that are ambiguous, do not
/// fit the face, do not add up to the declared count, have gaps, or differ between faces.
///
/// The checks that compare numbers, counts and faces run only while nothing else has been
/// reported: after a mistake, such as a value that could not be read, they would report the
/// same mistake again in other words.
pub fn number_faces(
    faces: &mut Faces,
    legend: &Legend,
    components: &Components,
    problems: &mut Vec<Problem>,
) {
    let parts = numbered_parts(legend, problems);
    // A legend value that fits no face, such as a wrong `layout=`, is reported once.
    let mut reported: Vec<SourceSpan> = Vec::new();
    for face in faces.iter_mut() {
        for numbered in &parts {
            let mut found = Vec::new();
            let shown = number_part(face, numbered, &mut found);
            for problem in found {
                if !reported.contains(&problem.span()) {
                    reported.push(problem.span());
                    problems.push(problem);
                }
            }
            if problems.is_empty() && !shown.is_empty() {
                check_numbers(face, numbered, &shown, components, problems);
            }
        }
    }
    if problems.is_empty() {
        for numbered in &parts {
            check_faces_agree(faces, numbered, problems);
        }
    }
}

/// Reports faces that split a part's numbers differently between its groups. Complete tables
/// can only differ there: the listed numbers are the same on every face, and the free ones
/// count up the same way.
fn check_faces_agree(faces: &Faces, numbered: &NumberedPart<'_>, problems: &mut Vec<Problem>) {
    let sizes = |numbers: &Numbers| -> Vec<usize> {
        numbers.segments.iter().map(|segment| segment.size).collect()
    };
    let mut tables = faces.iter().filter_map(|face| Some((face, face.numbers(numbered.part)?)));
    let Some((first, reference)) = tables.next() else { return };
    let expected = sizes(reference);
    for (face, numbers) in tables {
        let found = sizes(numbers);
        if found == expected {
            continue;
        }
        let groups = numbered.groups.iter().zip(&found);
        let found: Vec<String> =
            groups.map(|(group, &size)| group.describe(numbered.part, Some(size))).collect();
        let expected: Vec<String> = expected.iter().map(ToString::to_string).collect();
        problems.push(
            Problem::new(
                format!(
                    "the {} face shows {}, but the {} face shows {}",
                    face.kind.name(),
                    found.join(" and "),
                    first.kind.name(),
                    expected.join(" and ")
                ),
                face.span,
            )
            .with_label_at(first.span, format!("the {} face", first.kind.name())),
        );
    }
}

/// Sorts the numbered legend entries by part and group, and, while nothing else has been
/// reported, reports a group with two entries without `numbers=`, or a number listed twice
/// on the same media.
fn numbered_parts<'l>(legend: &'l Legend, problems: &mut Vec<Problem>) -> Vec<NumberedPart<'l>> {
    let check = problems.is_empty();
    let mut parts: Vec<NumberedPart<'l>> = Vec::new();
    for entry in legend.entries().filter(|entry| entry.part.kind() == PartKind::List) {
        let at = parts.iter().position(|numbered| numbered.part == entry.part);
        let at = at.unwrap_or_else(|| {
            parts.push(NumberedPart { part: entry.part, groups: Vec::new() });
            parts.len() - 1
        });
        let numbered = &mut parts[at];
        let name = entry.numbering.group.as_deref();
        let at = numbered.groups.iter().position(|group| group.name == name);
        let at = at.unwrap_or_else(|| {
            numbered.groups.push(Group { name, entries: Vec::new(), listed: Vec::new() });
            numbered.groups.len() - 1
        });
        let group = &mut numbered.groups[at];
        if check {
            check_entry(numbered.part, group, entry, problems);
        }
        group.entries.push(entry);
    }
    for group in parts.iter_mut().flat_map(|numbered| &mut numbered.groups) {
        let lists = group.entries.iter().filter_map(|entry| entry.numbering.numbers.as_ref());
        group.listed = lists.flat_map(|numbers| numbers.value.iter().copied()).collect();
        group.listed.sort_unstable();
        group.listed.dedup();
    }
    parts
}

/// Checks a legend entry against the entries of its group before it: only one of them may
/// go without `numbers=`, and only an RJ45 and an SFP entry may list the same number.
fn check_entry(part: Part, group: &Group<'_>, entry: &LegendEntry, problems: &mut Vec<Problem>) {
    let Some(numbers) = &entry.numbering.numbers else {
        if let Some(other) = group.free() {
            problems.push(
                Problem::new(
                    format!(
                        "`{}` and `{}` both number the {} from `first`",
                        other.key,
                        entry.key,
                        group.describe(part, None)
                    ),
                    entry.span,
                )
                .with_help(
                    "give all but one of them `numbers=`; the one without takes the numbers left",
                ),
            );
        }
        return;
    };
    let mut sorted = numbers.value.clone();
    sorted.sort_unstable();
    for other in group.entries.iter().filter(|other| other.media == entry.media) {
        let Some(listed) = &other.numbering.numbers else { continue };
        let mut shared: Vec<u16> = listed
            .value
            .iter()
            .copied()
            .filter(|number| sorted.binary_search(number).is_ok())
            .collect();
        if shared.is_empty() {
            continue;
        }
        shared.sort_unstable();
        let mut problem = Problem::new(
            format!(
                "`{}` and `{}` give the same numbers to the {}: {}",
                other.key,
                entry.key,
                group.describe(part, None),
                describe_runs(
                    shared.chunk_by(|a, b| b - a == 1).map(|run| (run[0], run[run.len() - 1]))
                )
            ),
            numbers.span,
        );
        if part.has_media() {
            problem = problem
                .with_help("only an RJ45 and an SFP element may share a number, as one combo port");
        }
        problems.push(problem);
    }
}

/// Numbers the elements of one part on `face`, and reports a face showing only some of the
/// part's entries. Returns the entries the face shows.
fn number_part<'l>(
    face: &mut Face,
    numbered: &NumberedPart<'l>,
    problems: &mut Vec<Problem>,
) -> Vec<Shown<'l>> {
    let mut shown: Vec<Shown<'l>> = Vec::new();
    let mut missing: Vec<char> = Vec::new();
    for (group, entries) in numbered.groups.iter().enumerate() {
        for &entry in &entries.entries {
            let elements = ordered(face, entry, problems);
            if elements.is_empty() {
                missing.push(entry.key);
            } else {
                shown.push(Shown { group, entry, elements });
            }
        }
    }
    if shown.is_empty() {
        return shown;
    }
    if !missing.is_empty() {
        let (name, _) = names(numbered.part);
        let keys: Vec<String> = missing.iter().map(|key| format!("`{key}`")).collect();
        problems.push(
            Problem::new(
                format!("the {} face shows {name}s, but not {}", face.kind.name(), keys.join(", ")),
                face.span,
            )
            .with_help(format!("a face shows every legend key of its {name}s, or none of them")),
        );
    }
    for shown in &shown {
        give_numbers(face, &numbered.groups[shown.group], shown, problems);
    }
    shown
}

/// Checks the numbers of one part on `face` against the declared count and for gaps, then
/// keeps the part's number table on the face, one segment per group.
fn check_numbers(
    face: &mut Face,
    numbered: &NumberedPart<'_>,
    shown: &[Shown<'_>],
    components: &Components,
    problems: &mut Vec<Problem>,
) {
    // Each group's numbers, sorted, with the entry and element that have them. The earlier
    // checks leave them distinct, but for the two elements of a combo.
    let mut sorted: Vec<Vec<(u16, &LegendEntry, usize)>> = vec![Vec::new(); numbered.groups.len()];
    for shown in shown {
        let elements = shown.elements.iter();
        let numbers = elements.filter_map(|&index| Some((face.elements()[index].number?, index)));
        sorted[shown.group].extend(numbers.map(|(number, index)| (number, shown.entry, index)));
    }
    for numbers in &mut sorted {
        numbers.sort_unstable_by_key(|&(number, ..)| number);
    }
    let slots =
        |numbers: &[(u16, &LegendEntry, usize)]| numbers.chunk_by(|a, b| a.0 == b.0).count();
    let total: usize = sorted.iter().map(|numbers| slots(numbers)).sum();
    let declared = components.count(numbered.part);
    if total != usize::from(declared) {
        let (name, node) = names(numbered.part);
        let count = if declared == 0 { "none".to_owned() } else { declared.to_string() };
        let mut problem = Problem::new(
            format!(
                "the {} face shows {}, but the model declares {count}",
                face.kind.name(),
                plural(total, name)
            ),
            face.span,
        );
        if declared == 0 {
            problem = problem.with_help(format!("add `{node} {total}` to the model"));
        }
        problems.push(problem);
        return;
    }
    let mut complete = true;
    for (group, numbers) in numbered.groups.iter().zip(&sorted) {
        complete &= !report_gaps(face, numbered.part, group, numbers, problems);
    }
    if !complete {
        return;
    }

    // Without gaps, each group's numbers run from its lowest one, one slot per number.
    let mut table = Numbers { part: numbered.part, segments: Vec::new(), slots: Vec::new() };
    for (group, numbers) in numbered.groups.iter().zip(&sorted) {
        let Some(&(base, ..)) = numbers.first() else { continue };
        let (offset, size) = (table.slots.len(), slots(numbers));
        table.segments.push(Segment { group: group.name.map(str::to_owned), base, offset, size });
        for elements in numbers.chunk_by(|a, b| a.0 == b.0) {
            let mut slot = Slot::default();
            for &(_, entry, index) in elements {
                if entry.media == Some(Media::Sfp) {
                    slot.sfp = Some(index);
                } else {
                    slot.element = Some(index);
                }
            }
            table.slots.push(slot);
        }
    }
    face.add_numbers(table);
}

/// Gives the elements of an entry on `face` their numbers: those its `numbers=` lists, or
/// else the numbers its group leaves, counting up from its `first`. Reports numbers that do
/// not fit the elements.
fn give_numbers(
    face: &mut Face,
    group: &Group<'_>,
    shown: &Shown<'_>,
    problems: &mut Vec<Problem>,
) {
    let Shown { entry, elements, .. } = shown;
    if let Some(numbers) = &entry.numbering.numbers {
        for (&index, &number) in elements.iter().zip(&numbers.value) {
            face.set_number(index, number);
        }
        if numbers.value.len() != elements.len() {
            problems.push(Problem::new(
                format!(
                    "`numbers` lists {}, but the {} face has {}",
                    plural(numbers.value.len(), "number"),
                    face.kind.name(),
                    plural(elements.len(), &format!("`{}` element", entry.key)),
                ),
                numbers.span,
            ));
        }
        return;
    }
    let left = (entry.numbering.first..=u16::MAX)
        .filter(|number| group.listed.binary_search(number).is_err());
    let mut given = 0;
    for (&index, number) in elements.iter().zip(left) {
        face.set_number(index, number);
        given += 1;
    }
    if given < elements.len() {
        problems.push(Problem::new(
            format!("`{}` runs out of numbers after {}", entry.key, u16::MAX),
            entry.span,
        ));
    }
}

/// Reports the numbers a group skips between its sorted `numbers`. A gap is blamed on what
/// gives the number after it: a `numbers=` list, or the `first=` of the entry that takes
/// the numbers left. Returns whether it reported.
fn report_gaps(
    face: &Face,
    part: Part,
    group: &Group<'_>,
    numbers: &[(u16, &LegendEntry, usize)],
    problems: &mut Vec<Problem>,
) -> bool {
    let mut skipped: Vec<(u16, u16)> = Vec::new();
    let mut causes: Vec<SourceSpan> = Vec::new();
    for pair in numbers.windows(2).filter(|pair| pair[1].0 - pair[0].0 > 1) {
        skipped.push((pair[0].0 + 1, pair[1].0 - 1));
        let after = &pair[1].1.numbering;
        let cause = after.numbers.as_ref().map(|numbers| numbers.span).or(after.first_span);
        let cause = cause.unwrap_or(pair[1].1.span);
        if !causes.contains(&cause) {
            causes.push(cause);
        }
    }
    let Some((cause, others)) = causes.split_first() else { return false };
    let mut problem = Problem::new(
        format!(
            "the numbers of the {} on the {} face skip {}",
            group.describe(part, None),
            face.kind.name(),
            describe_runs(skipped)
        ),
        *cause,
    );
    for &other in others {
        problem = problem.with_label_at(other, "this leaves a gap too");
    }
    problems.push(problem);
    true
}

/// Returns the name of a numbered part in messages, and the node declaring its count.
const fn names(part: Part) -> (&'static str, &'static str) {
    match part {
        Part::Bay => ("bay", "bays"),
        Part::Psu => ("PSU", "psus"),
        Part::Nic => ("NIC", "nics"),
        Part::Mgmt => ("management port", "mgmt"),
        Part::Outlet => ("outlet", "outlets"),
        _ => ("port", "ports"),
    }
}

/// Writes runs of numbers, given as their first and last numbers, such as `3, 7-9`.
fn describe_runs(runs: impl IntoIterator<Item = (u16, u16)>) -> String {
    let run = |(from, to): (u16, u16)| {
        if from == to { from.to_string() } else { format!("{from}-{to}") }
    };
    runs.into_iter().map(run).collect::<Vec<_>>().join(", ")
}

/// Returns the elements of `entry`'s key on `face`, as positions in [`Face::elements`], in
/// the order they are numbered. A `layout=` that does not match the face is reported, and
/// the grid of the picture is used instead.
pub fn ordered(face: &Face, entry: &LegendEntry, problems: &mut Vec<Problem>) -> Vec<usize> {
    let mut positions = picture_grid(face, entry.key);
    if let Some(layout) = &entry.numbering.layout
        && let Some(placed) = layout_grid(face, entry.key, layout, &positions, problems)
    {
        positions = placed;
    }
    let (primary, secondary) = (entry.numbering.order.primary, entry.numbering.order.secondary);
    let up = primary == Direction::Up || secondary == Some(Direction::Up);
    let left = primary == Direction::Left || secondary == Some(Direction::Left);
    let along = |a: usize, b: usize, reverse: bool| if reverse { b.cmp(&a) } else { a.cmp(&b) };
    positions.sort_by(|a, b| {
        let (rows, columns) = (along(a.row, b.row, up), along(a.column, b.column, left));
        if primary.is_horizontal() { rows.then(columns) } else { columns.then(rows) }
    });
    positions.into_iter().map(|position| position.index).collect()
}

/// Places the elements of `key` as the picture shows them: in their picture rows, where
/// the k-th element of a row is in column k, whatever lies between them.
fn picture_grid(face: &Face, key: char) -> Vec<Position> {
    let mut positions: Vec<Position> = Vec::new();
    // The elements are in reading order, so each row's elements come together.
    for (index, element) in face.elements().iter().enumerate() {
        if element.key != key {
            continue;
        }
        let column = match positions.last() {
            Some(last) if last.row == element.row => last.column + 1,
            _ => 0,
        };
        positions.push(Position { index, row: element.row, column });
    }
    positions
}

/// Places the elements of `key` on the grid of `layout`: the n-th row of the layout holds
/// the n-th picture row of these elements, and each `x` one of them, in order. Returns
/// `None` after reporting a layout that does not match the picture.
fn layout_grid(
    face: &Face,
    key: char,
    layout: &Spanned<Vec<Vec<bool>>>,
    picture: &[Position],
    problems: &mut Vec<Problem>,
) -> Option<Vec<Position>> {
    // A face without these elements has nothing for the layout to describe.
    if picture.is_empty() {
        return None;
    }
    let face_name = face.kind.name();
    let rows: Vec<&[Position]> = picture.chunk_by(|a, b| a.row == b.row).collect();
    if rows.len() != layout.value.len() {
        problems.push(Problem::new(
            format!(
                "`layout` has {}, but the {face_name} face has `{key}` elements on {}",
                plural(layout.value.len(), "row"),
                plural(rows.len(), "row"),
            ),
            layout.span,
        ));
        return None;
    }
    let mut placed = Vec::with_capacity(picture.len());
    for (row, (elements, slots)) in rows.iter().zip(&layout.value).enumerate() {
        let columns: Vec<usize> =
            slots.iter().enumerate().filter(|(_, slot)| **slot).map(|(column, _)| column).collect();
        if columns.len() != elements.len() {
            problems.push(Problem::new(
                format!(
                    "row {} of `layout` has {} `x`, but the {face_name} face has {} there",
                    row + 1,
                    columns.len(),
                    plural(elements.len(), &format!("`{key}` element")),
                ),
                layout.span,
            ));
            return None;
        }
        placed.extend(elements.iter().zip(columns).map(|(element, column)| Position {
            index: element.index,
            row,
            column,
        }));
    }
    Some(placed)
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};
    use miette::Diagnostic;

    use super::*;
    use crate::catalog::Model;
    use crate::testing::{covered, pointed};

    /// The text of a 1U server with the part counts `counts`, such as `bays 2`, whose normal
    /// face is `rows`, described by `legend` and a `c` identify LED.
    fn server(rows: [&str; 2], counts: &str, legend: &str) -> String {
        let [top, bottom] = rows;
        formatdoc! {r##"
            model {{ name "X"; kind "server"; {counts}
            face #"""
            {top}
            {bottom}
            """#
            legend {{ {legend}; c id }} }}"##}
    }

    /// Returns the key and number of each numbered element, in reading order.
    fn numbered(rows: [&str; 2], counts: &str, legend: &str) -> Vec<(char, u16)> {
        let model = Model::parse("x/y", &server(rows, counts, legend)).expect("valid model");
        let face = model.faces.normal.expect("normal face");
        face.elements().iter().filter_map(|element| Some((element.key, element.number?))).collect()
    }

    /// Asserts the problems of a server model, each as its message and the text it points at.
    #[track_caller]
    fn assert_reported(rows: [&str; 2], counts: &str, legend: &str, expected: &[(&str, &str)]) {
        let text = server(rows, counts, legend);
        let problems = Model::parse("x/y", &text).expect_err("problems");
        assert_eq!(pointed(&text, &problems), expected);
    }

    /// Returns the row and column of each `b` element, in numbering order.
    fn order(rows: [&str; 2], counts: &str, entry: &str) -> Vec<(usize, usize)> {
        let model = Model::parse("x/y", &server(rows, counts, entry)).expect("valid model");
        let face = model.faces.normal.expect("normal face");
        let entry = model.legend.get('b').expect("legend entry");
        let mut problems = Vec::new();
        let order = ordered(&face, entry, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        order
            .iter()
            .map(|&index| (face.elements()[index].row, face.elements()[index].column))
            .collect()
    }

    #[test]
    fn numbers_along_rows_or_down_columns_from_any_corner() {
        let grid = ["b b bc", "b b bc"];
        let by = |direction: &str| order(grid, "bays 6", &format!("b bay order=\"{direction}\""));
        assert_eq!(by("right"), [(0, 0), (0, 2), (0, 4), (1, 0), (1, 2), (1, 4)]);
        assert_eq!(by("left"), [(0, 4), (0, 2), (0, 0), (1, 4), (1, 2), (1, 0)]);
        assert_eq!(by("right-up"), [(1, 0), (1, 2), (1, 4), (0, 0), (0, 2), (0, 4)]);
        assert_eq!(by("left-up"), [(1, 4), (1, 2), (1, 0), (0, 4), (0, 2), (0, 0)]);
        assert_eq!(by("down"), [(0, 0), (1, 0), (0, 2), (1, 2), (0, 4), (1, 4)]);
        assert_eq!(by("up"), [(1, 0), (0, 0), (1, 2), (0, 2), (1, 4), (0, 4)]);
        assert_eq!(by("down-left"), [(0, 4), (1, 4), (0, 2), (1, 2), (0, 0), (1, 0)]);
        assert_eq!(by("up-left"), [(1, 4), (0, 4), (1, 2), (0, 2), (1, 0), (0, 0)]);
        assert_eq!(order(grid, "bays 6", "b bay"), by("right"));
    }

    #[test]
    fn counts_columns_by_the_elements_of_the_key() {
        // The second row is shifted and has a `c` LED first, but its first `b` is still in
        // the first column.
        let order = order(["b b bc", " c b b b"], "bays 6", r#"b bay order="down""#);
        assert_eq!(order, [(0, 0), (1, 3), (0, 2), (1, 5), (0, 4), (1, 7)]);
    }

    #[test]
    fn numbers_on_the_grid_a_layout_gives() {
        // The R630's 3 + 5 bays: the top row starts in the third column.
        let rows = ["  b b bc", "b b b b b"];
        assert_eq!(
            order(rows, "bays 8", r#"b bay order="down" layout="--xxx,xxxxx""#),
            [(1, 0), (1, 2), (0, 2), (1, 4), (0, 4), (1, 6), (0, 6), (1, 8)]
        );
        assert_eq!(
            order(rows, "bays 8", r#"b bay order="down""#),
            [(0, 2), (1, 0), (0, 4), (1, 2), (0, 6), (1, 4), (1, 6), (1, 8)]
        );
    }

    #[test]
    fn reports_a_layout_that_does_not_match_the_picture() {
        assert_reported(
            ["b bc", "b b"],
            "bays 4",
            r#"b bay layout="xx""#,
            &[(
                "`layout` has 1 row, but the normal face has `b` elements on 2 rows",
                r#"layout="xx""#,
            )],
        );
        assert_reported(
            ["b bc", "b b"],
            "bays 4",
            r#"b bay layout="xx,x""#,
            &[(
                "row 2 of `layout` has 1 `x`, but the normal face has 2 `b` elements there",
                r#"layout="xx,x""#,
            )],
        );
    }

    #[test]
    fn numbers_in_order_from_first() {
        assert_eq!(
            numbered(["b b bc", "b b b"], "bays 6", r#"b bay first=0 order="down""#),
            [('b', 0), ('b', 2), ('b', 4), ('b', 1), ('b', 3), ('b', 5)]
        );
    }

    #[test]
    fn gives_the_numbers_left_to_the_entry_without_numbers() {
        assert_eq!(
            numbered(["xoo xoc", "o o"], "outlets 7", r#"x outlet numbers="1,4"; o outlet"#),
            [('x', 1), ('o', 2), ('o', 3), ('x', 4), ('o', 5), ('o', 6), ('o', 7)]
        );
        // The numbers left can start below the listed ones.
        assert_eq!(
            numbered(["xooc", "~"], "outlets 3", r#"x outlet numbers="1"; o outlet first=0"#),
            [('x', 1), ('o', 0), ('o', 2)]
        );
    }

    #[test]
    fn numbers_and_looks_up_combo_ports_and_groups() {
        // `g` and `s` are the combo ports 5 and 6, and port 7 is an SFP cage alone. The `x`
        // ports are XG1 and XG2, so the switch has 7 + 2 ports.
        let rows = ["nn g s sxc", "nn g s x"];
        let legend = r#"n port
            g port numbers="5-6" order="down"
            s port media="sfp" numbers="5-7" order="down"
            x port group="XG""#;
        assert_eq!(
            numbered(rows, "ports 9", legend),
            [
                ('n', 1),
                ('n', 2),
                ('g', 5),
                ('s', 5),
                ('s', 7),
                ('x', 1),
                ('n', 3),
                ('n', 4),
                ('g', 6),
                ('s', 6),
                ('x', 2)
            ]
        );

        let text = server(rows, "ports 9", legend);
        let face = Model::parse("x/y", &text).expect("valid model").faces.normal.expect("face");
        let numbers = face.numbers(Part::Port).expect("port numbers");
        // The key and row of each element with a number.
        let at = |index: Option<usize>| {
            index.map(|at| (face.elements()[at].key, face.elements()[at].row))
        };
        let get =
            |group, number| numbers.get(group, number).map(|slot| (at(slot.element), at(slot.sfp)));
        assert_eq!(get(None, 1), Some((Some(('n', 0)), None)));
        assert_eq!(get(None, 6), Some((Some(('g', 1)), Some(('s', 1)))));
        assert_eq!(get(None, 7), Some((None, Some(('s', 0)))));
        assert_eq!(get(Some("XG"), 2), Some((Some(('x', 1)), None)));
        assert_eq!(get(None, 8), None);
        assert_eq!(get(Some("XG"), 0), None);
        assert_eq!(get(Some("YY"), 1), None);
        assert!(face.numbers(Part::Bay).is_none());
        assert!(numbers.get(None, 6).is_some_and(Slot::is_combo));
        assert!(!numbers.get(None, 7).is_some_and(Slot::is_combo));
        assert_eq!(
            [None, Some("XG"), Some("YY")].map(|group| numbers.base(group)),
            [Some(1), Some(1), None]
        );

        // A combo counts once, and the groups count together.
        let picture = indoc! {r##"
            #"""
            nn g s sxc
            nn g s x
            """#"##};
        assert_reported(
            rows,
            "ports 11",
            legend,
            &[("the normal face shows 9 ports, but the model declares 11", picture)],
        );
    }

    #[test]
    fn places_endpoints_across_groups() {
        // Ports 1 to 7, then XG1 and XG2.
        let rows = ["nn g s sxc", "nn g s x"];
        let legend = r#"n port
            g port numbers="5-6" order="down"
            s port media="sfp" numbers="5-7" order="down"
            x port group="XG""#;
        let model = Model::parse("x/y", &server(rows, "ports 9", legend)).expect("valid model");
        let index = |group, number| model.endpoint_index(Part::Port, group, number);
        assert_eq!(
            [index(None, 1), index(None, 7), index(Some("XG"), 1)],
            [Some(0), Some(6), Some(7)]
        );
        assert_eq!([index(None, 0), index(None, 8), index(Some("XG"), 3)], [None, None, None]);
        assert_eq!(model.endpoint_name(Part::Port, 8), Some((Some("XG"), 2)));
        assert_eq!(model.endpoint_name(Part::Port, 9), None);
    }

    #[test]
    fn places_endpoints_numbered_from_first() {
        let model = Model::parse("x/y", &server(["n n nc", "n n n"], "ports 6", "n port first=0"))
            .expect("valid model");
        let index = |number| model.endpoint_index(Part::Port, None, number);
        assert_eq!([index(0), index(5), index(6)], [Some(0), Some(5), None]);
        assert_eq!(model.endpoint_name(Part::Port, 0), Some((None, 0)));
    }

    #[test]
    fn places_endpoints_the_face_does_not_show_from_1() {
        let model =
            Model::parse("x/y", &server(["b bc", "~"], "bays 2; psus 2", "b bay")).expect("valid");
        let index = |group, number| model.endpoint_index(Part::Psu, group, number);
        assert_eq!([index(None, 1), index(None, 2)], [Some(0), Some(1)]);
        assert_eq!([index(None, 0), index(None, 3), index(Some("XG"), 1)], [None, None, None]);
        assert_eq!(model.endpoint_name(Part::Psu, 1), Some((None, 2)));
        assert_eq!(model.endpoint_runs(Part::Nic).count(), 0);
    }

    #[test]
    fn numbers_combos_of_any_network_connector() {
        let legend = r#"l nic numbers="1-2"; s nic media="sfp" numbers="2""#;
        assert_eq!(numbered(["l l s", "~c"], "nics 2", legend), [('l', 1), ('l', 2), ('s', 2)]);
    }

    #[test]
    fn reports_two_entries_taking_the_numbers_left_once() {
        // The count is not checked either, as the numbers overlap.
        assert_reported(
            ["xoo xoc", "o o"],
            "",
            "x outlet; o outlet",
            &[("`x` and `o` both number the outlets from `first`", "o outlet")],
        );
    }

    #[test]
    fn reports_a_number_listed_twice_on_the_same_media() {
        assert_reported(
            ["g hc", "n"],
            "ports 2",
            r#"g port numbers="1"; h port numbers="1"; n port"#,
            &[("`g` and `h` give the same numbers to the ports: 1", r#"numbers="1""#)],
        );
    }

    #[test]
    fn reports_numbers_that_do_not_fit_the_elements() {
        assert_reported(
            ["x xoc", "o o"],
            "outlets 5",
            r#"x outlet numbers="1,2,3"; o outlet"#,
            &[(
                "`numbers` lists 3 numbers, but the normal face has 2 `x` elements",
                r#"numbers="1,2,3""#,
            )],
        );
        assert_reported(
            ["b bc", "~"],
            "bays 2",
            "b bay first=65535; ~ fill",
            &[("`b` runs out of numbers after 65535", "b bay first=65535")],
        );
    }

    #[test]
    fn reports_a_count_the_face_does_not_show() {
        let picture = indoc! {r##"
            #"""
            b bc
            b b
            """#"##};
        assert_reported(
            ["b bc", "b b"],
            "bays 3",
            "b bay",
            &[("the normal face shows 4 bays, but the model declares 3", picture)],
        );
        let text = server(["b bc", "b b"], "", "b bay");
        let problems = Model::parse("x/y", &text).expect_err("no count");
        assert_eq!(
            problems[0].message(),
            "the normal face shows 4 bays, but the model declares none"
        );
        assert_eq!(problems[0].help(), Some("add `bays 4` to the model"));
    }

    #[test]
    fn reports_a_face_showing_only_some_keys_of_a_part() {
        let text = r#"model {
            name "X"; kind "switch"; ports 3
            face "n n g\n~"
            face compact=#true "n n"
            legend { n port; g port numbers="3"; ~ fill }
        }"#;
        let problems = Model::parse("x/y", text).expect_err("some keys");
        assert_eq!(
            pointed(text, &problems),
            [("the compact face shows ports, but not `g`", r#""n n""#)]
        );
        assert_eq!(
            problems[0].help(),
            Some("a face shows every legend key of its ports, or none of them")
        );
    }

    #[test]
    fn reports_faces_that_split_the_numbers_differently() {
        let text = r#"model {
            name "X"; kind "switch"; ports 6
            face "nnnn xx\n~"
            face compact=#true "nnn xxx"
            legend { n port; x port group="XG"; ~ fill }
        }"#;
        let problems = Model::parse("x/y", text).expect_err("faces differ");
        assert_eq!(
            pointed(text, &problems),
            [(
                "the compact face shows 3 ports and 3 ports in group `XG`, but the normal face \
                 shows 4 and 2",
                r#""nnn xxx""#
            )]
        );
    }

    #[test]
    fn reports_a_value_that_cannot_be_read_once() {
        // Without its `numbers=`, `x` would also take the numbers left, as `o` does.
        assert_reported(
            ["xxooc", "~"],
            "outlets 4",
            r#"x outlet numbers="1,x"; o outlet; ~ fill"#,
            &[(
                "`numbers` must be numbers and ranges such as `1,9,17` or `25-28`, found `1,x`",
                r#"numbers="1,x""#,
            )],
        );
        // Read as RJ45, `s` would repeat the number of `g`.
        assert_reported(
            ["g sc", "~"],
            "ports 1",
            r#"g port numbers="1"; s port media="fibre" numbers="1"; ~ fill"#,
            &[("`media` must be one of `rj45`, `sfp`, found `fibre`", r#"media="fibre""#)],
        );
        assert_reported(
            ["bbc", "~"],
            "bays \"2\"",
            "b bay; ~ fill",
            &[("`number of bays` must be a whole number, found the string \"2\"", "\"2\"")],
        );
        // The `o` cells are drawn as they are, so the face shows one outlet of three.
        assert_reported(
            ["xooc", "~"],
            "outlets 3",
            r#"x outlet numbers="1"; o outlets; ~ fill"#,
            &[("unknown legend type `outlets`", "outlets")],
        );
    }

    #[test]
    fn reports_a_mistake_shared_by_two_faces_once() {
        let model = |numbers: &str| {
            format!(
                r#"model {{
                    name "X"; kind "pdu"; outlets 3
                    face "x xo\n~"
                    face compact=#true "x xo"
                    legend {{ x outlet numbers="{numbers}"; o outlet; ~ fill }}
                }}"#
            )
        };
        let text = model("1,9");
        assert_eq!(
            pointed(&text, &Model::parse("x/y", &text).expect_err("gap")),
            [("the numbers of the outlets on the normal face skip 3-8", r#"numbers="1,9""#)]
        );
        let text = model("1,2,3");
        assert_eq!(
            pointed(&text, &Model::parse("x/y", &text).expect_err("too many numbers")),
            [(
                "`numbers` lists 3 numbers, but the normal face has 2 `x` elements",
                r#"numbers="1,2,3""#
            )]
        );
    }

    #[test]
    fn reports_gaps_at_what_gives_the_number_after_them() {
        // 9 comes from `x`'s list, after the 2-4 that `o` takes.
        assert_reported(
            ["x xoc", "o o"],
            "outlets 5",
            r#"x outlet numbers="1,9"; o outlet"#,
            &[("the numbers of the outlets on the normal face skip 5-8", r#"numbers="1,9""#)],
        );
        // 5 comes from `o`, which counts up from `first=5`.
        assert_reported(
            ["xxooc", "~"],
            "outlets 4",
            r#"x outlet numbers="1,2"; o outlet first=5; ~ fill"#,
            &[("the numbers of the outlets on the normal face skip 3-4", "first=5")],
        );
        assert_reported(
            ["g hc", "~"],
            "ports 2",
            r#"g port numbers="1" group="XG"; h port numbers="3" group="XG"; ~ fill"#,
            &[(
                "the numbers of the ports in group `XG` on the normal face skip 2",
                r#"numbers="3""#,
            )],
        );
        // Two gaps with two causes: the list gives 4, and `first=7` gives 7.
        let text =
            server(["xxooc", "~"], "outlets 4", r#"x outlet numbers="1,4"; o outlet first=7"#);
        let problems = Model::parse("x/y", &text).expect_err("gaps");
        assert_eq!(
            pointed(&text, &problems),
            [("the numbers of the outlets on the normal face skip 2-3, 5-6", r#"numbers="1,4""#)]
        );
        let labels: Vec<_> = problems[0]
            .labels()
            .expect("labels")
            .map(|label| {
                (label.label().unwrap_or_default().to_owned(), covered(&text, *label.inner()))
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("here".to_owned(), r#"numbers="1,4""#),
                ("this leaves a gap too".to_owned(), "first=7")
            ]
        );
    }
}
