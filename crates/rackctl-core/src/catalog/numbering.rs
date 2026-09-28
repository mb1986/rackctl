//! Numbering: the numbers of a face's numbered elements, such as bays and ports.

use super::face::{Face, Faces};
use super::legend::{Direction, Legend, LegendEntry, Media, Part, PartKind};
use super::model::Components;
use crate::kdl_reader::{Problem, Spanned};

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
struct Numbering<'l> {
    part: Part,
    groups: Vec<Group<'l>>,
    /// Whether the legend numbers the part ambiguously or lists a number twice, which has
    /// been reported. The faces are then numbered, but not checked.
    reported: bool,
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

    /// Names the group's elements in messages, such as `outlets` or ``ports in group `XG` ``.
    fn describe(&self, part: Part) -> String {
        let (name, _) = names(part);
        self.name.map_or_else(|| format!("{name}s"), |group| format!("{name}s in group `{group}`"))
    }
}

/// The elements that have one number: one element, or for a combo port an RJ45 and an SFP
/// element.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    /// The RJ45 element of a network connector, or the element of any other part.
    element: Option<usize>,
    /// The SFP element of a network connector.
    sfp: Option<usize>,
}

impl Slot {
    const fn is_empty(self) -> bool {
        self.element.is_none() && self.sfp.is_none()
    }
}

/// Numbers the elements of every face, and reports numberings that are ambiguous, do not
/// fit the face, do not add up to the declared count, or have gaps.
pub fn number_faces(
    faces: &mut Faces,
    legend: &Legend,
    components: &Components,
    problems: &mut Vec<Problem>,
) {
    let numberings = numberings(legend, problems);
    for face in faces.iter_mut() {
        for numbering in &numberings {
            number_part(face, numbering, components, problems);
        }
    }
}

/// Sorts the numbered legend entries by part and group, and reports a group with two
/// entries without `numbers=`, or a number listed twice on the same media.
fn numberings<'l>(legend: &'l Legend, problems: &mut Vec<Problem>) -> Vec<Numbering<'l>> {
    let mut numberings: Vec<Numbering<'l>> = Vec::new();
    for entry in legend.entries().filter(|entry| entry.part.kind() == PartKind::List) {
        let at = numberings.iter().position(|numbering| numbering.part == entry.part);
        let at = at.unwrap_or_else(|| {
            numberings.push(Numbering { part: entry.part, groups: Vec::new(), reported: false });
            numberings.len() - 1
        });
        let numbering = &mut numberings[at];
        let name = entry.numbering.group.as_deref();
        let at = numbering.groups.iter().position(|group| group.name == name);
        let at = at.unwrap_or_else(|| {
            numbering.groups.push(Group { name, entries: Vec::new(), listed: Vec::new() });
            numbering.groups.len() - 1
        });
        let group = &mut numbering.groups[at];
        let before = problems.len();
        check_entry(numbering.part, group, entry, problems);
        group.entries.push(entry);
        numbering.reported |= problems.len() > before;
    }
    for group in numberings.iter_mut().flat_map(|numbering| &mut numbering.groups) {
        let lists = group.entries.iter().filter_map(|entry| entry.numbering.numbers.as_ref());
        group.listed = lists.flat_map(|numbers| numbers.value.iter().copied()).collect();
        group.listed.sort_unstable();
        group.listed.dedup();
    }
    numberings
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
                        group.describe(part)
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
    for other in group.entries.iter().filter(|other| other.media == entry.media) {
        let Some(listed) = &other.numbering.numbers else { continue };
        let mut shared: Vec<u16> =
            numbers.value.iter().copied().filter(|number| listed.value.contains(number)).collect();
        if shared.is_empty() {
            continue;
        }
        shared.sort_unstable();
        let mut problem = Problem::new(
            format!(
                "`{}` and `{}` give the same numbers to the {}: {}",
                other.key,
                entry.key,
                group.describe(part),
                describe_runs(&runs(shared.iter().map(|&number| usize::from(number))))
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

/// Numbers the elements of one part on `face`, then checks them: a face shows all of the
/// part's entries or none, each `numbers=` fits its elements, the numbers add up to the
/// declared count, and they leave no gaps.
fn number_part(
    face: &mut Face,
    numbering: &Numbering<'_>,
    components: &Components,
    problems: &mut Vec<Problem>,
) {
    // The elements of each entry the face shows, in numbering order, with the entry's group.
    let mut shown: Vec<(usize, &LegendEntry, Vec<usize>)> = Vec::new();
    let mut missing: Vec<char> = Vec::new();
    for (at, group) in numbering.groups.iter().enumerate() {
        for &entry in &group.entries {
            let order = ordered(face, entry, problems);
            if order.is_empty() {
                missing.push(entry.key);
            } else {
                shown.push((at, entry, order));
            }
        }
    }
    if shown.is_empty() {
        return;
    }
    if !missing.is_empty() {
        let (name, _) = names(numbering.part);
        let keys: Vec<String> = missing.iter().map(|key| format!("`{key}`")).collect();
        problems.push(
            Problem::new(
                format!("the {} face shows {name}s, but not {}", face.kind.name(), keys.join(", ")),
                face.span,
            )
            .with_help(format!("a face shows every legend key of its {name}s, or none of them")),
        );
    }
    let mut consistent = missing.is_empty() && !numbering.reported;
    for (group, entry, order) in &shown {
        consistent &= give_numbers(face, &numbering.groups[*group], entry, order, problems);
    }
    if consistent {
        check_numbers(face, numbering, &shown, components, problems);
    }
}

/// Checks the numbers of one part on `face` against the declared count, then fills the
/// part's number table, one segment per group, and reports the numbers it skips.
fn check_numbers(
    face: &Face,
    numbering: &Numbering<'_>,
    shown: &[(usize, &LegendEntry, Vec<usize>)],
    components: &Components,
    problems: &mut Vec<Problem>,
) {
    // The size of each group's segment: its listed numbers, and the elements of its free
    // entry, which take one new number each.
    let mut sizes: Vec<usize> = numbering.groups.iter().map(|group| group.listed.len()).collect();
    for (group, entry, order) in shown {
        if entry.numbering.numbers.is_none() {
            sizes[*group] += order.len();
        }
    }
    let total: usize = sizes.iter().sum();
    let declared = usize::from(declared(components, numbering.part));
    let (name, node) = names(numbering.part);
    if total != declared {
        let face_name = face.kind.name();
        let problem = if declared == 0 {
            Problem::new(
                format!(
                    "the {face_name} face shows {}, but the model declares none",
                    plural(total, name)
                ),
                face.span,
            )
            .with_help(format!("add `{node} {total}` to the model"))
        } else {
            Problem::new(
                format!(
                    "the {face_name} face shows {}, but the model declares {declared}",
                    plural(total, name)
                ),
                face.span,
            )
        };
        problems.push(problem);
        return;
    }

    let mut table = vec![Slot::default(); total];
    let mut offset = 0;
    for (at, (group, size)) in numbering.groups.iter().zip(sizes).enumerate() {
        let segment = &mut table[offset..offset + size];
        offset += size;
        let first = group.free().map(|entry| entry.numbering.first);
        let Some(base) = first.into_iter().chain(group.listed.first().copied()).min() else {
            continue;
        };
        // Numbers past the end of the segment, which leave a gap before them.
        let mut beyond: Vec<usize> = Vec::new();
        for (_, entry, order) in shown.iter().filter(|(group, ..)| *group == at) {
            for &index in order {
                let Some(number) = face.elements()[index].number else { continue };
                let Some(slot) = segment.get_mut(usize::from(number - base)) else {
                    beyond.push(usize::from(number));
                    continue;
                };
                if entry.media == Some(Media::Sfp) {
                    slot.sfp = Some(index);
                } else {
                    slot.element = Some(index);
                }
            }
        }
        report_gaps(face, numbering.part, group, usize::from(base), segment, beyond, problems);
    }
}

/// Gives the elements of `entry` on `face` their numbers: those its `numbers=` lists, or
/// else the numbers its group leaves, counting up from its `first`. Returns `false` after
/// reporting numbers that do not fit the elements.
fn give_numbers(
    face: &mut Face,
    group: &Group<'_>,
    entry: &LegendEntry,
    order: &[usize],
    problems: &mut Vec<Problem>,
) -> bool {
    if let Some(numbers) = &entry.numbering.numbers {
        for (&index, &number) in order.iter().zip(&numbers.value) {
            face.set_number(index, number);
        }
        if numbers.value.len() == order.len() {
            return true;
        }
        problems.push(Problem::new(
            format!(
                "`numbers` lists {}, but the {} face has {}",
                plural(numbers.value.len(), "number"),
                face.kind.name(),
                plural(order.len(), &format!("`{}` element", entry.key)),
            ),
            numbers.span,
        ));
        return false;
    }
    let left = (entry.numbering.first..=u16::MAX)
        .filter(|number| group.listed.binary_search(number).is_err());
    let mut numbered = 0;
    for (&index, number) in order.iter().zip(left) {
        face.set_number(index, number);
        numbered += 1;
    }
    if numbered == order.len() {
        return true;
    }
    problems.push(Problem::new(
        format!("`{}` runs out of numbers after {}", entry.key, u16::MAX),
        entry.span,
    ));
    false
}

/// Reports the numbers a group's segment skips: its empty slots, and the numbers between
/// its end and the numbers `beyond` it. Only `numbers=` can leave a gap, as the numbers
/// left count up without one, so the problem points at them.
fn report_gaps(
    face: &Face,
    part: Part,
    group: &Group<'_>,
    base: usize,
    segment: &[Slot],
    mut beyond: Vec<usize>,
    problems: &mut Vec<Problem>,
) {
    let empty = segment.iter().enumerate().filter(|(_, slot)| slot.is_empty());
    let mut skipped = runs(empty.map(|(at, _)| base + at));
    beyond.sort_unstable();
    let mut next = base + segment.len();
    for number in beyond {
        if number > next {
            add_run(&mut skipped, next, number - 1);
        }
        next = number + 1;
    }
    let lists = group.entries.iter().filter_map(|entry| entry.numbering.numbers.as_ref());
    let lists: Vec<&Spanned<Vec<u16>>> = lists.collect();
    let Some((first, others)) = lists.split_first().filter(|_| !skipped.is_empty()) else {
        return;
    };
    let mut problem = Problem::new(
        format!(
            "the numbers of the {} on the {} face skip {}",
            group.describe(part),
            face.kind.name(),
            describe_runs(&skipped)
        ),
        first.span,
    );
    for other in others {
        problem = problem.with_label_at(other.span, "numbers also given here");
    }
    problems.push(problem);
}

/// Returns the declared count of a numbered part, such as `bays 8`; zero when absent.
const fn declared(components: &Components, part: Part) -> u16 {
    match part {
        Part::Bay => components.bays,
        Part::Psu => components.psus,
        Part::Nic => components.nics,
        Part::Mgmt => components.mgmt,
        Part::Port => components.ports,
        Part::Outlet => components.outlets,
        _ => 0,
    }
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

/// Groups sorted numbers into runs of consecutive ones.
fn runs(numbers: impl IntoIterator<Item = usize>) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for number in numbers {
        add_run(&mut runs, number, number);
    }
    runs
}

/// Adds the run `from..=to` after `runs`, joining it to the last run when they touch.
fn add_run(runs: &mut Vec<(usize, usize)>, from: usize, to: usize) {
    match runs.last_mut() {
        Some((_, end)) if *end + 1 == from => *end = to,
        _ => runs.push((from, to)),
    }
}

/// Writes runs of numbers, such as `3, 7-9`.
fn describe_runs(runs: &[(usize, usize)]) -> String {
    let run = |&(from, to): &(usize, usize)| {
        if from == to { from.to_string() } else { format!("{from}-{to}") }
    };
    runs.iter().map(run).collect::<Vec<_>>().join(", ")
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

/// Writes a count with its noun, such as `1 row` or `3 rows`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

    use super::*;
    use crate::catalog::Model;
    use crate::testing::pointed;

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

    /// The problems of a server model, with the text each points at.
    fn reported(rows: [&str; 2], counts: &str, legend: &str) -> Vec<(String, String)> {
        let text = server(rows, counts, legend);
        let problems = Model::parse("x/y", &text).expect_err("problems");
        pointed(&text, &problems).into_iter().map(|(a, b)| (a.to_owned(), b.to_owned())).collect()
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
        let text = server(["b bc", "b b"], "bays 4", r#"b bay layout="xx""#);
        assert_eq!(
            pointed(&text, &Model::parse("x/y", &text).expect_err("wrong rows")),
            [(
                "`layout` has 1 row, but the normal face has `b` elements on 2 rows",
                r#"layout="xx""#
            )]
        );
        let text = server(["b bc", "b b"], "bays 4", r#"b bay layout="xx,x""#);
        assert_eq!(
            pointed(&text, &Model::parse("x/y", &text).expect_err("wrong elements")),
            [(
                "row 2 of `layout` has 1 `x`, but the normal face has 2 `b` elements there",
                r#"layout="xx,x""#
            )]
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
    }

    #[test]
    fn numbers_combo_ports_and_groups() {
        // `g` and `s` are the combo ports 5 and 6; the `x` ports are XG1 and XG2. That makes
        // 6 ports in the default group and 2 in `XG`.
        let legend = r#"n port
            g port numbers="5-6" order="down"
            s port media="sfp" numbers="5-6" order="down"
            x port group="XG""#;
        assert_eq!(
            numbered(["nn g s xc", "nn g s x"], "ports 8", legend),
            [
                ('n', 1),
                ('n', 2),
                ('g', 5),
                ('s', 5),
                ('x', 1),
                ('n', 3),
                ('n', 4),
                ('g', 6),
                ('s', 6),
                ('x', 2)
            ]
        );
    }

    #[test]
    fn numbers_combos_of_any_network_connector() {
        let legend = r#"l nic numbers="1-2"; s nic media="sfp" numbers="2""#;
        assert_eq!(numbered(["l l s", "~c"], "nics 2", legend), [('l', 1), ('l', 2), ('s', 2)]);
    }

    #[test]
    fn reports_two_entries_taking_the_numbers_left_once() {
        // The count is not checked either, as the numbers overlap.
        let problems = reported(["xoo xoc", "o o"], "", "x outlet; o outlet");
        assert_eq!(
            problems,
            [(
                "`x` and `o` both number the outlets from `first`".to_owned(),
                "o outlet".to_owned()
            )]
        );
    }

    #[test]
    fn reports_numbers_that_do_not_fit_the_face() {
        let owned = |message: &str, at: &str| (message.to_owned(), at.to_owned());
        assert_eq!(
            reported(["x xoc", "o o"], "outlets 5", r#"x outlet numbers="1,2,3"; o outlet"#),
            [owned(
                "`numbers` lists 3 numbers, but the normal face has 2 `x` elements",
                r#"numbers="1,2,3""#
            )]
        );
        assert_eq!(
            reported(["x xoc", "o o"], "outlets 5", r#"x outlet numbers="1,9"; o outlet"#),
            [owned("the numbers of the outlets on the normal face skip 5-8", r#"numbers="1,9""#)]
        );
        assert_eq!(
            reported(["g hc", "n"], "ports 2", r#"g port numbers="1"; h port numbers="1"; n port"#),
            [owned("`g` and `h` give the same numbers to the ports: 1", r#"numbers="1""#)]
        );
        assert_eq!(
            reported(["b bc", "~"], "bays 2", "b bay first=65535; ~ fill"),
            [owned("`b` runs out of numbers after 65535", "b bay first=65535")]
        );
    }

    #[test]
    fn reports_a_count_the_face_does_not_show() {
        let picture = indoc! {r##"
            #"""
            b bc
            b b
            """#"##};
        let owned = |message: &str| (message.to_owned(), picture.to_owned());
        assert_eq!(
            reported(["b bc", "b b"], "bays 3", "b bay"),
            [owned("the normal face shows 4 bays, but the model declares 3")]
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
}
