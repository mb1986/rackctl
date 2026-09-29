//! A device's panel: its face in a frame.

use rackctl_core::catalog::{Ears, FaceKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use super::FaceView;
use crate::ui::theme::{EAR, PANEL, UNDERLINE};

/// A device's panel: its face between two ears and underlined, or a strip's face in a thin
/// frame.
#[derive(Clone, Copy)]
pub struct Panel<'a> {
    pub face: FaceView<'a>,
}

impl Panel<'_> {
    /// Returns the panel's width: the face and its frame.
    #[must_use]
    pub fn width(&self) -> u16 {
        u16::try_from(self.face.layout.width() + 2).unwrap_or(u16::MAX)
    }

    /// Returns the panel's height: the face, and for a strip, its frame.
    #[must_use]
    pub fn height(&self) -> u16 {
        let frame = if self.is_strip() { 2 } else { 0 };
        u16::try_from(self.face.layout.height() + frame).unwrap_or(u16::MAX)
    }

    fn is_strip(&self) -> bool {
        self.face.face.kind == FaceKind::Strip
    }
}

impl Widget for Panel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(Rect { width: self.width(), height: self.height(), ..area });
        if area.width < 2 || area.height == 0 {
            return;
        }
        if self.is_strip() {
            if area.height >= 2 {
                render_strip(self.face, area, buf);
            }
        } else {
            render_ears(self.face, area, buf);
        }
    }
}

/// Draws a strip's face in a thin frame.
fn render_strip(face: FaceView<'_>, area: Rect, buf: &mut Buffer) {
    face.render(
        Rect { x: area.x + 1, y: area.y + 1, width: area.width - 2, height: area.height - 2 },
        buf,
    );
    let style = Style::new().fg(EAR).bg(PANEL);
    let inside = usize::from(area.width - 2);
    buf.set_string(area.x, area.y, format!("🭽{}🭾", "▔".repeat(inside)), style);
    buf.set_string(area.x, area.bottom() - 1, format!("🭼{}🭿", "▁".repeat(inside)), style);
    for y in area.top() + 1..area.bottom() - 1 {
        buf.set_string(area.x, y, "▏", style);
        buf.set_string(area.right() - 1, y, "▕", style);
    }
}

/// Draws a face between two ears, underlined on its last row.
fn render_ears(face: FaceView<'_>, area: Rect, buf: &mut Buffer) {
    face.render(Rect { x: area.x + 1, width: area.width - 2, ..area }, buf);
    let style = Style::new().fg(EAR).bg(PANEL);
    let rows = usize::from(area.height);
    for (row, y) in (area.top()..area.bottom()).enumerate() {
        let (left, right) = ears(face.model.ears, row, rows);
        for (x, ear) in [(area.left(), left), (area.right() - 1, right)] {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(ear).set_style(style);
            }
        }
    }
    let underline = Style::new().add_modifier(Modifier::UNDERLINED).underline_color(UNDERLINE);
    for x in area.left()..area.right() {
        if let Some(cell) = buf.cell_mut((x, area.bottom() - 1)) {
            cell.set_style(underline);
        }
    }
}

/// Returns the left and right ear of a row.
const fn ears(ears: Ears, row: usize, rows: usize) -> (char, char) {
    let last = row + 1 == rows;
    match ears {
        Ears::Screws if row == 0 || last => ('⊕', '⊕'),
        Ears::Screws => (' ', ' '),
        Ears::Heavy if rows == 1 => ('╸', '╺'),
        Ears::Heavy if row == 0 => ('┓', '┏'),
        Ears::Heavy if last => ('┛', '┗'),
        Ears::Heavy => ('┃', '┃'),
    }
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use rackctl_core::catalog::{Model, State};

    use super::*;
    use crate::ui::art::{FaceLayout, Look};
    use crate::ui::theme::Tone;

    /// Paints the panel of a model with `extra` nodes and a face of `rows`, 4 columns wide.
    fn paint(extra: &str, rows: &str) -> Buffer {
        let text = formatdoc! {r##"
            model {{ name "X"; kind "server"; {extra}
            face #"""
            {rows}
            """#
            legend {{ p power }} }}"##};
        let model = Model::parse("x/y", &text).expect("valid model");
        let face = model.faces.normal.as_ref().expect("normal face");
        let layout = FaceLayout::new(face, 4);
        let look = |_| Look { state: State::On, tone: Tone::Good };
        let view = FaceView {
            model: &model,
            face,
            layout: &layout,
            name: "",
            amps: "",
            look: &look,
            numbers: false,
        };
        let panel = Panel { face: view };
        let mut buf = Buffer::empty(Rect::new(0, 0, panel.width(), panel.height()));
        panel.render(buf.area, &mut buf);
        buf
    }

    fn lines(buf: &Buffer) -> Vec<String> {
        let area = buf.area;
        (area.top()..area.bottom())
            .map(|y| (area.left()..area.right()).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn frames_the_face_with_ears() {
        assert_eq!(lines(&paint("height 2", "p\n\n\n")), ["┓●   ┏", "┃    ┃", "┃    ┃", "┛    ┗"]);
        assert_eq!(lines(&paint("", "p\n")), ["┓●   ┏", "┛    ┗"]);
        assert_eq!(ears(Ears::Heavy, 0, 1), ('╸', '╺'));
    }

    #[test]
    fn puts_screws_on_the_first_and_last_row() {
        let buf = paint(r#"height 2; ears "screws""#, "p\n\n\n");
        assert_eq!(lines(&buf), ["⊕●   ⊕", "      ", "      ", "⊕    ⊕"]);
    }

    #[test]
    fn tints_the_panel_and_underlines_its_last_row() {
        let buf = paint("", "p\n");
        let (ear, bottom) = (&buf[(0, 0)], &buf[(3, 1)]);
        assert_eq!((ear.fg, ear.bg), (EAR, PANEL));
        assert!(!ear.modifier.contains(Modifier::UNDERLINED));
        assert!(bottom.modifier.contains(Modifier::UNDERLINED));
        assert_eq!((bottom.bg, bottom.underline_color), (PANEL, UNDERLINE));
    }
}
