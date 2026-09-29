//! Painting a face.

use rackctl_core::catalog::{
    Align, Cell, Element, Face, LegendEntry, Model, Part, PartKind, State,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use super::FaceLayout;
use crate::ui::glyphs::default_glyph;
use crate::ui::theme::{LITERAL, PANEL, TEXT, Tone};

/// How an element with a state is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// Chooses the glyph.
    pub state: State,
    /// Chooses the colour.
    pub tone: Tone,
}

/// One device's face, ready to paint.
#[derive(Clone, Copy)]
pub struct FaceView<'a> {
    pub model: &'a Model,
    pub face: &'a Face,
    pub layout: &'a FaceLayout,
    /// The device's name, for `name` fields.
    pub name: &'a str,
    /// The total current, for `amps` fields.
    pub amps: &'a str,
    /// The look of each LED and numbered element, by position in [`Face::elements`].
    pub look: &'a dyn Fn(usize) -> Look,
    /// Whether numbered elements show their numbers instead of their glyphs.
    pub numbers: bool,
}

/// Blank panel, and text fields.
const BASE: Style = Style::new().fg(TEXT).bg(PANEL);

impl Widget for FaceView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let elements = self.face.elements();
        let shown: Vec<(Vec<char>, Style)> =
            elements.iter().enumerate().map(|(index, element)| self.show(index, element)).collect();
        for (y, row) in (area.y..area.bottom()).zip(self.layout.rows()) {
            let (row, columns) = row.unwrap_or((0, &[]));
            for (x, at) in (area.x..area.right()).zip(0..self.layout.width()) {
                let column = columns.get(at).copied().flatten();
                let (ch, style) = match column.and_then(|at| Some((at, self.face.cell(row, at)?))) {
                    Some((_, Cell::Literal(ch))) => (ch, BASE.fg(LITERAL)),
                    Some((at, Cell::Element(index))) => {
                        let element = &elements[index];
                        let (chars, style) = &shown[index];
                        let cell = (row - element.row) * element.width + at - element.column;
                        (chars.get(cell).copied().unwrap_or(' '), *style)
                    }
                    None => (' ', BASE),
                };
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch).set_style(style);
                }
            }
        }
    }
}

impl FaceView<'_> {
    /// Returns an element's characters, one per cell in reading order, and its style.
    fn show(&self, index: usize, element: &Element) -> (Vec<char>, Style) {
        let entry = self.model.legend.get(element.key);
        let cells = element.width * element.height;
        match element.part.kind() {
            PartKind::Led | PartKind::List => {
                let look = (self.look)(index);
                let style = BASE.fg(look.tone.color());
                if self.numbers && element.part.kind() == PartKind::List {
                    let number =
                        element.number.map(|number| number.to_string()).unwrap_or_default();
                    let width = element.width;
                    let mut chars: Vec<char> =
                        format!("{:>width$}", last(&number, width)).chars().collect();
                    chars.resize(cells, ' ');
                    return (chars, style);
                }
                let media = entry.and_then(|entry| entry.media);
                let glyph = entry
                    .and_then(|entry| entry.glyph(look.state))
                    .unwrap_or_else(|| default_glyph(element.part, media, look.state));
                let chars: Vec<char> = glyph.chars().collect();
                let chars = if chars.len() == 1 { vec![chars[0]; cells] } else { chars };
                (chars, style)
            }
            PartKind::Text => {
                let style = match element.part {
                    Part::Name => BASE.add_modifier(Modifier::BOLD),
                    Part::Number => BASE.fg(LITERAL),
                    _ => BASE,
                };
                (self.text(element, entry).chars().collect(), style)
            }
            PartKind::Layout => (Vec::new(), BASE),
        }
    }

    /// Returns a text field's text, fitted to its width.
    fn text(&self, element: &Element, entry: Option<&LegendEntry>) -> String {
        let align = entry.and_then(|entry| entry.align);
        let value = match element.part {
            Part::Number => return self.number_field(element, entry),
            Part::Name => self.name,
            Part::Short => &self.model.short,
            Part::Model => &self.model.name,
            Part::Amps => self.amps,
            _ => entry.and_then(|entry| entry.text.as_deref()).unwrap_or_default(),
        };
        fit(value, element.width, align.unwrap_or(Align::Left))
    }

    /// Returns a number field's text: the number of the element it touches, next to its gap.
    fn number_field(&self, field: &Element, entry: Option<&LegendEntry>) -> String {
        let Some(numbered) = field.number_of.map(|at| &self.face.elements()[at]) else {
            return " ".repeat(field.width);
        };
        let gap = entry.map_or(0, |entry| entry.gap);
        let room = field.width.saturating_sub(gap);
        let number = numbered.number.map(|number| number.to_string()).unwrap_or_default();
        // Aligned towards its element by default.
        let left_of = numbered.column > field.column;
        let lean = if left_of { Align::Right } else { Align::Left };
        let label =
            fit(last(&number, room), room, entry.and_then(|entry| entry.align).unwrap_or(lean));
        let gap = " ".repeat(gap);
        if left_of { label + &gap } else { gap + &label }
    }
}

/// Fits `value` to `width` columns: aligned, or cut with `…` when longer.
fn fit(value: &str, width: usize, align: Align) -> String {
    if value.chars().count() > width {
        return value.chars().take(width.saturating_sub(1)).chain(['…']).take(width).collect();
    }
    match align {
        Align::Left => format!("{value:<width$}"),
        Align::Right => format!("{value:>width$}"),
        Align::Center => format!("{value:^width$}"),
    }
}

/// Returns the last `count` characters of an ASCII `text`, such as the last digits of a number.
fn last(text: &str, count: usize) -> &str {
    &text[text.len().saturating_sub(count)..]
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use ratatui::style::Color;

    use super::*;

    /// A 1U server named `Dell PowerEdge` (`R630`), with `counts` and the picture `rows`.
    fn server(rows: [&str; 2], counts: &str, legend: &str) -> Model {
        let [top, bottom] = rows;
        let text = formatdoc! {r##"
            model {{ name "Dell PowerEdge"; short "R630"; kind "server"; {counts}
            face #"""
            {top}
            {bottom}
            """#
            legend {{ {legend} }} }}"##};
        Model::parse("x/y", &text).expect("valid model")
    }

    /// Paints the model's normal face, `width` columns wide, as the device `srv01`.
    fn paint(model: &Model, width: u16, numbers: bool, look: impl Fn(&Element) -> Look) -> Buffer {
        let face = model.faces.normal.as_ref().expect("normal face");
        let layout = FaceLayout::new(face, usize::from(width));
        let look = |index: usize| look(&face.elements()[index]);
        let view = FaceView {
            model,
            face,
            layout: &layout,
            name: "srv01",
            amps: "4.1A",
            look: &look,
            numbers,
        };
        let height = u16::try_from(face.rows().count()).expect("few rows");
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        view.render(buf.area, &mut buf);
        buf
    }

    fn lines(buf: &Buffer) -> Vec<String> {
        let area = buf.area;
        (area.top()..area.bottom())
            .map(|y| (area.left()..area.right()).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    const GOOD: Look = Look { state: State::Ok, tone: Tone::Good };

    #[test]
    fn draws_glyphs_in_the_colour_of_their_state() {
        let model = server(
            ["p b__ ll s", "~"],
            "bays 1; nics 2; ports 1",
            r#"p power; b bay="[■]"; l nic; s port media="sfp"; ~ fill"#,
        );
        let buf = paint(&model, 12, false, |element| match (element.key, element.number) {
            ('p', _) => Look { state: State::On, tone: Tone::Good },
            ('l', Some(2)) => Look { state: State::Down, tone: Tone::Dim },
            ('l' | 's', _) => Look { state: State::Up, tone: Tone::Link },
            _ => GOOD,
        });
        assert_eq!(lines(&buf), ["● [■] ▣□ ▬  ", "            "]);
        let (literal, down, panel) = (&buf[(1, 0)], &buf[(7, 0)], &buf[(0, 1)]);
        assert_eq!((literal.fg, literal.bg), (LITERAL, PANEL));
        assert_eq!((down.fg, down.bg), (Color::Indexed(240), PANEL));
        assert_eq!(panel.bg, PANEL);
    }

    #[test]
    fn picks_the_legend_glyph_for_the_state() {
        let model = server(["b__ b__", "~"], "bays 2", r#"b bay="[■]" empty="[ ]"; ~ fill"#);
        let buf = paint(&model, 7, false, |element| match element.number {
            Some(2) => Look { state: State::Empty, tone: Tone::Dim },
            _ => GOOD,
        });
        assert_eq!(lines(&buf)[0], "[■] [ ]");
    }

    #[test]
    fn fits_text_fields_to_their_runs() {
        let model = server(
            ["nnnnnnn hhhh ttttt", "mmmmmmmmmm aaaaa"],
            "",
            r#"n name; h short; t text="Hello World"; m model; a amps align="right""#,
        );
        let buf = paint(&model, 18, false, |_| GOOD);
        assert_eq!(lines(&buf), ["srv01   R630 Hell…", "Dell Powe…  4.1A  "]);
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn shows_the_last_digits_of_a_number_next_to_its_element() {
        let model =
            server(["##b b##", "~"], "bays 2", r##""#" number gap=1; b bay first=10; ~ fill"##);
        let buf = paint(&model, 7, false, |_| GOOD);
        assert_eq!(lines(&buf)[0], "0 ■ ■ 1");
        assert_eq!(buf[(0, 0)].fg, LITERAL);
    }

    #[test]
    fn shows_numbers_instead_of_glyphs() {
        let model = server(["b__ b", "|||"], "bays 2", "b bay");
        assert_eq!(lines(&paint(&model, 5, true, |_| GOOD)), ["  1 2", "     "]);
    }
}
