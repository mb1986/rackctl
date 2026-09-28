//! Face legends: what the characters of a face picture stand for.

use std::collections::BTreeSet;

use kdl::{KdlDocument, KdlEntry, KdlIdentifier, KdlNode, NodeKey};
use miette::SourceSpan;
use strum::{EnumString, IntoStaticStr, VariantNames};

use crate::kdl_reader::{NodeReader, Problem, Spanned};

/// The legend of a model's faces: one entry per key character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legend {
    entries: Vec<LegendEntry>,
    /// The position in `entries` of each key's entry, indexed by the key's ASCII code, so
    /// that looking up a picture's characters takes constant time.
    index: [Option<u8>; 128],
}

impl Default for Legend {
    fn default() -> Self {
        Self { entries: Vec::new(), index: [None; 128] }
    }
}

impl Legend {
    /// Returns the entry for `key`, if the legend has one.
    #[must_use]
    pub fn get(&self, key: char) -> Option<&LegendEntry> {
        let code = u8::try_from(key).ok()?;
        let position = (*self.index.get(usize::from(code))?)?;
        self.entries.get(usize::from(position))
    }

    /// Returns the entries in the order they are written.
    pub fn entries(&self) -> impl Iterator<Item = &LegendEntry> {
        self.entries.iter()
    }

    /// Adds an entry whose key is ASCII and not yet in the legend.
    fn push(&mut self, entry: LegendEntry) {
        let (Ok(code), Ok(position)) = (u8::try_from(entry.key), u8::try_from(self.entries.len()))
        else {
            return;
        };
        if let Some(slot) = self.index.get_mut(usize::from(code)) {
            *slot = Some(position);
            self.entries.push(entry);
        }
    }
}

/// What one key character of a face picture stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegendEntry {
    /// The character used in the picture.
    pub key: char,
    /// The part of the device the key draws.
    pub part: Part,
    /// How the elements of a list part are numbered.
    pub numbering: Numbering,
    /// The fixed text of a `text` part.
    pub text: Option<String>,
    /// How a text or number part is placed in its run, when the entry chooses. Text is
    /// placed on the left by default, and a number towards the element it belongs to.
    pub align: Option<Align>,
    /// The blank columns a number part keeps next to its element.
    pub gap: usize,
    /// The socket type of an outlet, for example `C13`.
    pub outlet_type: Option<String>,
    /// The rated current of an outlet, for example `10A`.
    pub rating: Option<String>,
    /// Location of the entry in the model file.
    pub span: SourceSpan,
    glyph: Option<Spanned<String>>,
    state_glyphs: Vec<(State, Spanned<String>)>,
}

impl LegendEntry {
    /// Returns the glyph the model chooses for `state`: the entry's glyph for that state,
    /// else its general glyph, which then applies to every state. `None` leaves the choice
    /// to the renderer.
    #[must_use]
    pub fn glyph(&self, state: State) -> Option<&str> {
        self.state_glyphs
            .iter()
            .find(|(glyph_state, _)| *glyph_state == state)
            .map(|(_, glyph)| glyph)
            .or(self.glyph.as_ref())
            .map(|glyph| glyph.value.as_str())
    }

    /// Returns every glyph the entry gives, with its location in the model file.
    pub fn glyphs(&self) -> impl Iterator<Item = &Spanned<String>> {
        self.glyph.iter().chain(self.state_glyphs.iter().map(|(_, glyph)| glyph))
    }
}

/// What a legend key draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "kebab-case")]
pub enum Part {
    /// The power LED, which also shows the device's health.
    Power,
    /// The identify LED.
    Id,
    /// A drive bay.
    Bay,
    /// A power supply.
    Psu,
    /// A network interface of a server.
    Nic,
    /// An RJ45 port.
    Port,
    /// An SFP cage.
    Sfp,
    /// A power outlet.
    Outlet,
    /// The device's name.
    Name,
    /// The model's short name.
    Short,
    /// The model's full name.
    Model,
    /// A fixed text.
    Text,
    /// The total current of a PDU.
    Amps,
    /// The number of the numbered element the field touches.
    Number,
    /// Blank columns that stretch a row to the width of the face.
    Fill,
    /// One blank column.
    Space,
}

/// The groups of parts, which decide the options a legend entry takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartKind {
    /// A single LED.
    Led,
    /// Numbered elements, such as bays or ports.
    List,
    /// A text field.
    Text,
    /// Blank space.
    Layout,
}

/// A state an element is drawn in. Each part has its own set of states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "kebab-case")]
pub enum State {
    On,
    Standby,
    Off,
    Ok,
    Rebuilding,
    Failed,
    Empty,
    Up,
    Down,
}

impl Part {
    /// Returns the group of parts this one belongs to.
    #[must_use]
    pub const fn kind(self) -> PartKind {
        match self {
            Self::Power | Self::Id => PartKind::Led,
            Self::Bay | Self::Psu | Self::Nic | Self::Port | Self::Sfp | Self::Outlet => {
                PartKind::List
            }
            Self::Name | Self::Short | Self::Model | Self::Text | Self::Amps | Self::Number => {
                PartKind::Text
            }
            Self::Fill | Self::Space => PartKind::Layout,
        }
    }

    /// Returns the states the part can be drawn in.
    #[must_use]
    pub const fn states(self) -> &'static [State] {
        match self {
            Self::Power => &[State::On, State::Standby, State::Off],
            Self::Id | Self::Outlet => &[State::On, State::Off],
            Self::Bay => &[State::Ok, State::Rebuilding, State::Failed, State::Empty],
            Self::Psu => &[State::Ok, State::Failed, State::Off],
            Self::Nic | Self::Port | Self::Sfp => &[State::Up, State::Down],
            _ => &[],
        }
    }

    /// Returns whether the part is an RJ45 port or an SFP cage. Ports and cages of the same
    /// group share one numbering, so that a number used by both forms a combo port.
    #[must_use]
    pub const fn is_port(self) -> bool {
        matches!(self, Self::Port | Self::Sfp)
    }
}

/// How a text part is placed in its run.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Align {
    #[default]
    Left,
    Right,
    Center,
}

/// How the elements of a list part are numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbering {
    /// The number of the first element.
    pub first: u16,
    /// The order in which the elements are numbered.
    pub order: Order,
    /// The grid the elements are placed in, row by row, where `true` is an element and
    /// `false` an empty slot. When absent, the grid follows the picture.
    pub layout: Option<Spanned<Vec<Vec<bool>>>>,
    /// Explicit numbers for the elements, in numbering order.
    pub numbers: Option<Spanned<Vec<u16>>>,
    /// The port group the elements are numbered in, for example `XG`.
    pub group: Option<String>,
}

impl Default for Numbering {
    fn default() -> Self {
        Self { first: 1, order: Order::default(), layout: None, numbers: None, group: None }
    }
}

/// The order in which elements are numbered: along `primary`, then along `secondary`,
/// which is on the other axis. `up-left` numbers up each column, starting from the right.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Order {
    /// The direction elements are numbered in first.
    pub primary: Direction,
    /// The direction the rows or columns follow; by default down or to the right.
    pub secondary: Option<Direction>,
}

/// A direction in the picture.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Direction {
    #[default]
    Right,
    Left,
    Down,
    Up,
}

impl Direction {
    const fn is_horizontal(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }
}

/// The media an RJ45 or SFP port can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "kebab-case")]
enum Media {
    Rj45,
    Sfp,
}

/// Reads a `legend { ... }` node.
pub fn read_legend(node: &KdlNode, problems: &mut Vec<Problem>) -> Legend {
    let mut reader = NodeReader::new(node, problems);
    let block = reader.children();
    reader.finish();

    let mut legend = Legend::default();
    let mut keys: Vec<(char, SourceSpan)> = Vec::new();
    for child in block.map(KdlDocument::nodes).unwrap_or_default() {
        let Some(key) = read_key(child, problems) else { continue };
        let span = child.name().span();
        if let Some(&(_, first)) = keys.iter().find(|&&(other, _)| other == key) {
            problems.push(
                Problem::new(format!("legend key `{key}` is given more than once"), span)
                    .with_label("given again here")
                    .with_label_at(first, "first given here"),
            );
            continue;
        }
        keys.push((key, span));
        if let Some(entry) = read_entry(child, key, problems) {
            legend.push(entry);
        }
    }
    legend
}

/// Reads one legend entry, such as `p power` or `b bay="■" first=0`. Returns `None` when
/// its type is missing, unknown, not a string or given twice.
fn read_entry(node: &KdlNode, key: char, problems: &mut Vec<Problem>) -> Option<LegendEntry> {
    let (mut part, shorthand) = entry_type(node, key, problems)?;
    let mut reader = NodeReader::new(node, problems);
    let value = if let Some(name) = shorthand {
        reader.opt_str(name).map(|value| Spanned { value, span: span_of(node, name) })
    } else {
        // Marks the argument as read; `entry_type` has already checked it.
        let _ = reader.arg_str(0, "type");
        None
    };

    let mut invalid: Invalid = Vec::new();
    let mut glyph = None;
    let mut state_glyphs = Vec::new();
    let mut text = None;
    match part.kind() {
        PartKind::Led | PartKind::List => {
            glyph = value;
            for &state in part.states() {
                let name = <&str>::from(state);
                if let Some(value) = reader.opt_str(name) {
                    state_glyphs.push((state, Spanned { value, span: span_of(node, name) }));
                }
            }
        }
        PartKind::Text if part == Part::Text => match value {
            Some(value) => match check_text("a text", &value.value) {
                Ok(()) => text = Some(value.value),
                Err(message) => invalid.push((value.span, message)),
            },
            // A value of the wrong type has already been reported.
            None if shorthand.is_none() => invalid.push((
                node.span(),
                format!("a `text` entry needs its text, for example `{key} text=\"APC\"`"),
            )),
            None => {}
        },
        PartKind::Text | PartKind::Layout => {
            if let Some(value) = value {
                let type_name = <&str>::from(part);
                invalid.push((value.span, format!("`{type_name}` does not take a value")));
            }
        }
    }
    let align = if part.kind() == PartKind::Text { reader.opt_enum("align") } else { None };
    let gap = if part == Part::Number { reader.opt_int("gap").unwrap_or(0) } else { 0 };
    let numbering = if part.kind() == PartKind::List {
        read_numbering(node, &mut reader, part, &mut invalid)
    } else {
        Numbering::default()
    };
    if part == Part::Port && reader.opt_enum::<Media>("media") == Some(Media::Sfp) {
        part = Part::Sfp;
    }
    let (outlet_type, rating) = if part == Part::Outlet {
        (
            parsed(node, &mut reader, "type", |text| text_value("`type`", text), &mut invalid),
            parsed(node, &mut reader, "rating", |text| text_value("`rating`", text), &mut invalid),
        )
    } else {
        (None, None)
    };
    reader.finish();

    let entry = LegendEntry {
        key,
        part,
        numbering,
        text,
        align,
        gap,
        outlet_type: outlet_type.map(|value| value.value),
        rating: rating.map(|value| value.value),
        span: node.span(),
        glyph,
        state_glyphs,
    };
    for glyph in entry.glyphs() {
        if let Err(message) = check_text("a glyph", &glyph.value) {
            invalid.push((glyph.span, message));
        }
    }
    problems.extend(invalid.into_iter().map(|(span, message)| Problem::new(message, span)));
    Some(entry)
}

/// Finds the type of a legend entry: its argument, as in `p power`, or, in the shorthand
/// `b bay="■"`, the name of its first property, whose value is then the glyph, or the text
/// of a `text` entry. Returns the part and, for the shorthand, the name of that property.
fn entry_type<'n>(
    node: &'n KdlNode,
    key: char,
    problems: &mut Vec<Problem>,
) -> Option<(Part, Option<&'n str>)> {
    let has_argument = node.entries().iter().any(|entry| entry.name().is_none());
    let shorthand = node
        .entries()
        .first()
        .and_then(KdlEntry::name)
        .map(KdlIdentifier::value)
        .filter(|_| !has_argument);
    let type_name = match shorthand {
        Some(name) => name.to_owned(),
        None if has_argument => NodeReader::new(node, problems).arg_str(0, "type")?,
        None => {
            problems.push(
                Problem::new(format!("legend key `{key}` has no type"), node.span())
                    .with_help(format!("give it a type, for example `{key} bay`")),
            );
            return None;
        }
    };
    let Ok(part) = type_name.parse::<Part>() else {
        problems.push(unknown_type(node, key, &type_name, shorthand.is_some()));
        return None;
    };
    if has_argument && node.entry(type_name.as_str()).is_some() {
        problems.push(
            Problem::new(
                format!("the type of `{key}` is given twice"),
                span_of(node, type_name.as_str()),
            )
            .with_help(format!(
                "write it once: `{key} {type_name}` or `{key} {type_name}=\"...\"`"
            )),
        );
        return None;
    }
    Some((part, shorthand))
}

/// Reports an unknown legend type, with a hint at the likely intent.
fn unknown_type(node: &KdlNode, key: char, type_name: &str, shorthand: bool) -> Problem {
    let span = if shorthand { span_of(node, type_name) } else { span_of(node, 0) };
    let problem = Problem::new(format!("unknown legend type `{type_name}`"), span);
    if type_name == "health" {
        return problem.with_help("the power LED shows the health; use `power`");
    }
    // In the shorthand, a valid type written after an option, as in `b first=0 bay="■"`.
    let mut later =
        node.entries().iter().filter_map(KdlEntry::name).skip(1).map(KdlIdentifier::value);
    if let Some(found) = later.find(|name| name.parse::<Part>().is_ok()).filter(|_| shorthand) {
        return problem.with_help(format!("the type must come first: `{key} {found}=...`"));
    }
    problem.with_suggestion(type_name, Part::VARIANTS.iter().copied())
}

/// Checks a glyph or text written in a model file: it must not be empty or contain control
/// characters. `what` names it in the message, for example "a glyph".
fn check_text(what: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(format!("{what} must not be empty"))
    } else if value.chars().any(char::is_control) {
        Err(format!("{what} must not contain control characters"))
    } else {
        Ok(())
    }
}

/// Converts a text option, checked with [`check_text`].
fn text_value(what: &str, value: &str) -> Result<String, String> {
    check_text(what, value).map(|()| value.to_owned())
}

/// Invalid option values: where each one is and why it is invalid.
type Invalid = Vec<(SourceSpan, String)>;

/// Reads the numbering options of a list part.
fn read_numbering(
    node: &KdlNode,
    reader: &mut NodeReader<'_, '_>,
    part: Part,
    invalid: &mut Invalid,
) -> Numbering {
    let group = if part.is_port() {
        parsed(node, reader, "group", parse_group, invalid).map(|group| group.value)
    } else {
        None
    };
    Numbering {
        first: reader.opt_int("first").unwrap_or(1),
        order: parsed(node, reader, "order", parse_order, invalid)
            .map(|order| order.value)
            .unwrap_or_default(),
        layout: parsed(node, reader, "layout", parse_layout, invalid),
        numbers: parsed(node, reader, "numbers", parse_numbers, invalid),
        group,
    }
}

/// Reads the string option `key` and converts it with `parse`, recording why when it
/// cannot be converted.
fn parsed<T>(
    node: &KdlNode,
    reader: &mut NodeReader<'_, '_>,
    key: &str,
    parse: fn(&str) -> Result<T, String>,
    invalid: &mut Invalid,
) -> Option<Spanned<T>> {
    let text = reader.opt_str(key)?;
    let span = span_of(node, key);
    match parse(&text) {
        Ok(value) => Some(Spanned { value, span }),
        Err(message) => {
            invalid.push((span, message));
            None
        }
    }
}

/// Returns the location of an argument or property of `node`, or of the whole node when
/// the entry is absent.
fn span_of(node: &KdlNode, key: impl Into<NodeKey>) -> SourceSpan {
    node.entry(key).map_or_else(|| node.span(), KdlEntry::span)
}

/// Reads the key of a legend entry: a single ASCII letter or punctuation character.
fn read_key(node: &KdlNode, problems: &mut Vec<Problem>) -> Option<char> {
    let name = node.name().value();
    let span = node.name().span();
    // Keys such as a tab are shown escaped, so that the message stays readable.
    let shown = name.escape_debug();
    let mut chars = name.chars();
    let problem = match (chars.next(), chars.next()) {
        (Some(key), None) if is_key(key) => return Some(key),
        (Some('_' | '|'), None) => Problem::new(format!("`{name}` cannot be a legend key"), span)
            .with_help("`_` and `|` continue the field to the left and above in the picture"),
        (Some(_), None) => Problem::new(
            format!("legend key `{shown}` must be an ASCII letter or punctuation"),
            span,
        ),
        (None, _) => Problem::new("a legend key must not be empty", span),
        _ => Problem::new(format!("legend key `{shown}` must be a single character"), span),
    };
    problems.push(problem);
    None
}

/// A legend key is an ASCII letter or punctuation character, other than `_` and `|`, which
/// continue fields in the picture.
const fn is_key(key: char) -> bool {
    (key.is_ascii_alphabetic() || key.is_ascii_punctuation()) && !matches!(key, '_' | '|')
}

/// Parses an order such as `down` or `up-left`.
fn parse_order(text: &str) -> Result<Order, String> {
    let invalid = || {
        format!(
            "`order` must be `right`, `left`, `down` or `up`, optionally followed by a direction \
             on the other axis such as `up-left`, found `{text}`"
        )
    };
    let (primary, secondary) = text.split_once('-').map_or((text, None), |(a, b)| (a, Some(b)));
    let primary = primary.parse::<Direction>().map_err(|_| invalid())?;
    let secondary = secondary.map(str::parse::<Direction>).transpose().map_err(|_| invalid())?;
    if secondary.is_some_and(|secondary| secondary.is_horizontal() == primary.is_horizontal()) {
        return Err(invalid());
    }
    Ok(Order { primary, secondary })
}

/// Parses a layout such as `--xxx,xxxxx` into rows of slots.
fn parse_layout(text: &str) -> Result<Vec<Vec<bool>>, String> {
    let rows: Vec<Vec<bool>> = text
        .split(',')
        .map(|row| {
            row.chars()
                .map(|slot| match slot {
                    'x' => Ok(true),
                    '-' => Ok(false),
                    _ => Err(()),
                })
                .collect()
        })
        .collect::<Result<_, ()>>()
        .map_err(|()| format!("`layout` may only use `x`, `-` and `,`, found `{text}`"))?;
    if rows.iter().any(|row| !row.contains(&true)) {
        return Err(format!("every row of `layout` needs at least one `x`, found `{text}`"));
    }
    Ok(rows)
}

/// Parses numbers such as `25-28` or `1,9,17`.
fn parse_numbers(text: &str) -> Result<Vec<u16>, String> {
    let invalid = || {
        format!("`numbers` must be numbers and ranges such as `1,9,17` or `25-28`, found `{text}`")
    };
    let number = |digits: &str| {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        digits
            .parse::<u16>()
            .map_err(|_| format!("`numbers` goes up to {}, found {digits}", u16::MAX))
    };
    let mut numbers = Vec::new();
    let mut seen = BTreeSet::new();
    for part in text.split(',') {
        let (low, high) = part.split_once('-').unwrap_or((part, part));
        let (low, high) = (number(low)?, number(high)?);
        if low > high {
            return Err(invalid());
        }
        for number in low..=high {
            if !seen.insert(number) {
                return Err(format!("`numbers` lists {number} more than once"));
            }
            numbers.push(number);
        }
    }
    Ok(numbers)
}

/// Parses a group name, such as `XG`. It uses only ASCII letters, so that a port written
/// `XG1` cannot be read another way.
fn parse_group(text: &str) -> Result<String, String> {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphabetic()) {
        Ok(text.to_owned())
    } else {
        Err(format!("a group name uses only ASCII letters, for example `XG`, found `{text}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdl_reader;

    /// Parses `text` as the inside of a `legend { ... }` node.
    fn parse(text: &str) -> (Legend, Vec<Problem>) {
        let document = kdl_reader::parse(&format!("legend {{\n{text}\n}}")).expect("valid KDL");
        let mut problems = Vec::new();
        let legend = read_legend(&document.nodes()[0], &mut problems);
        (legend, problems)
    }

    fn entry(text: &str) -> LegendEntry {
        let (legend, problems) = parse(text);
        assert!(problems.is_empty(), "{problems:?}");
        legend.entries().next().expect("one entry").clone()
    }

    fn messages(text: &str) -> Vec<String> {
        parse(text).1.iter().map(|problem| problem.message().to_owned()).collect()
    }

    #[test]
    fn reads_the_type_alone_and_the_shorthand_with_a_value() {
        assert_eq!(entry("p power").part, Part::Power);
        assert_eq!(entry("b first=0 bay").numbering.first, 0);
        let bay = entry(r#"b bay="𜷝𜶺" first=0 order="down""#);
        assert_eq!(bay.part, Part::Bay);
        assert_eq!(bay.glyph(State::Ok), Some("𜷝𜶺"));
        assert_eq!(bay.numbering.first, 0);
        assert_eq!(bay.numbering.order, Order { primary: Direction::Down, secondary: None });
        assert_eq!(entry(r#"t text="APC""#).text.as_deref(), Some("APC"));
        assert_eq!(entry(r#"a amps align="right""#).align, Some(Align::Right));
        let number = entry(r##""#" number gap=1"##);
        assert_eq!((number.part, number.gap, number.align), (Part::Number, 1, None));
        assert_eq!(entry("~ fill").part, Part::Fill);
        assert_eq!(entry("* space").part, Part::Space);
    }

    #[test]
    fn reads_sfp_cages_written_either_way() {
        assert_eq!(entry(r#"s sfp="▬""#).part, Part::Sfp);
        assert_eq!(entry(r#"s port="▬" media="sfp""#).part, Part::Sfp);
        assert_eq!(entry(r#"n port media="rj45""#).part, Part::Port);
    }

    #[test]
    fn finds_entries_by_key() {
        let (legend, problems) = parse(r#"p power; b bay; ~ fill; t text="APC""#);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(legend.get('b').map(|entry| entry.part), Some(Part::Bay));
        assert_eq!(legend.get('~').map(|entry| entry.part), Some(Part::Fill));
        assert_eq!(legend.get('t').and_then(|entry| entry.text.as_deref()), Some("APC"));
        assert!(legend.get('x').is_none());
        assert!(legend.get('é').is_none());
        let keys: String = legend.entries().map(|entry| entry.key).collect();
        assert_eq!(keys, "pb~t");
    }

    #[test]
    fn returns_the_glyphs_the_model_chooses() {
        let port = entry(r#"n port down="□""#);
        assert_eq!((port.glyph(State::Up), port.glyph(State::Down)), (None, Some("□")));
        let outlet = entry(r#"x outlet="█" type="C19" rating="16A""#);
        assert_eq!((outlet.glyph(State::On), outlet.glyph(State::Off)), (Some("█"), Some("█")));
        assert_eq!(outlet.outlet_type.as_deref(), Some("C19"));
        let bay = entry(r#"b bay="■" empty="[ ]""#);
        assert_eq!((bay.glyph(State::Ok), bay.glyph(State::Empty)), (Some("■"), Some("[ ]")));
    }

    #[test]
    fn reads_numbering_options() {
        let text = r#"g port numbers="25-26,49" order="up-left" group="XG""#;
        let ports = entry(text);
        let numbers = ports.numbering.numbers.expect("numbers");
        assert_eq!(numbers.value, [25, 26, 49]);
        let offset = "legend {\n".len() + text.find("numbers=").expect("numbers in the text");
        assert_eq!(numbers.span.offset(), offset);
        assert!(ports.part.is_port());
        assert_eq!(
            ports.numbering.order,
            Order { primary: Direction::Up, secondary: Some(Direction::Left) }
        );
        assert_eq!(ports.numbering.group.as_deref(), Some("XG"));
        let bays = entry(r#"b bay layout="--xxx,xxxxx""#);
        assert_eq!(
            bays.numbering.layout.map(|layout| layout.value),
            Some(vec![vec![false, false, true, true, true], vec![true, true, true, true, true]])
        );
        let many = entry(r#"b bay numbers="0-65535""#).numbering.numbers.expect("numbers");
        assert_eq!(many.value.len(), 65536);
    }

    #[test]
    fn reports_values_of_the_wrong_type_once() {
        assert_eq!(
            messages("a 5; b bay=5; t text=5"),
            [
                "`type` must be a string, found the number 5",
                "`bay` must be a string, found the number 5",
                "`text` must be a string, found the number 5",
            ]
        );
    }

    #[test]
    fn explains_that_health_moved_to_the_power_led() {
        let (_, problems) = parse("h health");
        assert_eq!(problems[0].message(), "unknown legend type `health`");
        assert_eq!(problems[0].help(), Some("the power LED shows the health; use `power`"));
    }

    #[test]
    fn points_at_the_empty_glyph() {
        let text = r#"b bay="■" failed="""#;
        let (_, problems) = parse(text);
        assert_eq!(problems[0].message(), "a glyph must not be empty");
        let offset = "legend {\n".len() + text.find("failed=").expect("failed in the text");
        assert_eq!(problems[0].span().offset(), offset);
    }

    #[test]
    fn reports_unknown_types_and_misplaced_options() {
        let (_, problems) = parse(r#"b baay="■"; p power first=0; n nic group="XG"; t name="x""#);
        let found: Vec<_> = problems.iter().map(|p| (p.message(), p.help())).collect();
        assert_eq!(
            found,
            [
                ("unknown legend type `baay`", Some("did you mean `bay`?")),
                ("unknown property `first` on `p`", None),
                ("unknown property `group` on `n`", None),
                ("`name` does not take a value", None),
            ]
        );
    }

    #[test]
    fn suggests_state_names() {
        let (_, problems) = parse(r#"b bay faild="x""#);
        assert_eq!(problems[0].message(), "unknown property `faild` on `b`");
        assert_eq!(problems[0].help(), Some("did you mean `failed`?"));
    }

    #[test]
    fn reports_invalid_option_values() {
        assert_eq!(
            messages(
                r#"a bay order="right-left"
                   b bay order="sideways"
                   c bay layout="xx,x?"
                   d bay layout="xx,--"
                   e bay numbers="3-1"
                   f bay numbers="1,1"
                   g port group="G2"
                   h text=""
                   i bay="■" glyph="□"
                   j bay align="left" media="sfp"
                   k port media="usb"
                   l bay numbers="+1,+2"
                   m bay numbers="1, 2"
                   n bay numbers="65536"
                   o outlet type="" rating="1\n0A"
                   q text="A\tB""#
            ),
            [
                "`order` must be `right`, `left`, `down` or `up`, optionally followed by a \
                 direction on the other axis such as `up-left`, found `right-left`",
                "`order` must be `right`, `left`, `down` or `up`, optionally followed by a \
                 direction on the other axis such as `up-left`, found `sideways`",
                "`layout` may only use `x`, `-` and `,`, found `xx,x?`",
                "every row of `layout` needs at least one `x`, found `xx,--`",
                "`numbers` must be numbers and ranges such as `1,9,17` or `25-28`, found `3-1`",
                "`numbers` lists 1 more than once",
                "a group name uses only ASCII letters, for example `XG`, found `G2`",
                "a text must not be empty",
                "unknown property `glyph` on `i`",
                "unknown property `align` on `j`",
                "unknown property `media` on `j`",
                "`media` must be one of `rj45`, `sfp`, found `usb`",
                "`numbers` must be numbers and ranges such as `1,9,17` or `25-28`, found `+1,+2`",
                "`numbers` must be numbers and ranges such as `1,9,17` or `25-28`, found `1, 2`",
                "`numbers` goes up to 65535, found 65536",
                "`type` must not be empty",
                "`rating` must not contain control characters",
                "a text must not contain control characters",
            ]
        );
    }

    #[test]
    fn reports_invalid_and_repeated_keys() {
        assert_eq!(
            messages(
                r#"bb bay; bb bay; _ bay; | bay; " " bay; "\t" bay; "é" bay; "" bay; p power; p power"#
            ),
            [
                "legend key `bb` must be a single character",
                "legend key `bb` must be a single character",
                "`_` cannot be a legend key",
                "`|` cannot be a legend key",
                "legend key ` ` must be an ASCII letter or punctuation",
                "legend key `\\t` must be an ASCII letter or punctuation",
                "legend key `é` must be an ASCII letter or punctuation",
                "a legend key must not be empty",
                "legend key `p` is given more than once",
            ]
        );
    }

    #[test]
    fn reports_a_missing_or_repeated_type_once() {
        let (_, problems) = parse(r#"x; t text; u text text="APC"; v text="APC" text"#);
        let found: Vec<_> = problems.iter().map(|p| (p.message(), p.help())).collect();
        assert_eq!(
            found,
            [
                ("legend key `x` has no type", Some("give it a type, for example `x bay`")),
                ("a `text` entry needs its text, for example `t text=\"APC\"`", None),
                (
                    "the type of `u` is given twice",
                    Some("write it once: `u text` or `u text=\"...\"`")
                ),
                (
                    "the type of `v` is given twice",
                    Some("write it once: `v text` or `v text=\"...\"`")
                ),
            ]
        );
    }

    #[test]
    fn explains_that_the_type_comes_first() {
        let (_, problems) = parse(r#"b first=0 bay="■""#);
        assert_eq!(problems[0].message(), "unknown legend type `first`");
        assert_eq!(problems[0].help(), Some("the type must come first: `b bay=...`"));
    }

    #[test]
    fn lists_every_glyph_with_its_location() {
        let text = r#"n port="▣" down="□""#;
        let port = entry(text);
        let glyphs: Vec<_> = port
            .glyphs()
            .map(|glyph| (glyph.value.as_str(), glyph.span.offset() - "legend {\n".len()))
            .collect();
        let at = |needle: &str| text.find(needle).expect("in the text");
        assert_eq!(glyphs, [("▣", at("port=")), ("□", at("down="))]);
    }
}
