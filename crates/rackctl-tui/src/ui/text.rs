//! A buffer as lines of text, for the command line.

use std::fmt::Write;

use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};

/// Returns the buffer's rows as plain text.
#[must_use]
pub fn plain(buf: &Buffer) -> Vec<String> {
    rows(buf).map(|row| row.iter().map(Cell::symbol).collect()).collect()
}

/// Returns the buffer's rows as text with ANSI styles.
#[must_use]
pub fn ansi(buf: &Buffer) -> Vec<String> {
    rows(buf)
        .map(|row| {
            let mut line = String::new();
            for run in row.chunk_by(same_style) {
                let text: String = run.iter().map(Cell::symbol).collect();
                match codes(&run[0]) {
                    codes if codes.is_empty() => line.push_str(&text),
                    codes => {
                        let _ = write!(line, "\x1b[{codes}m{text}\x1b[0m");
                    }
                }
            }
            line
        })
        .collect()
}

fn rows(buf: &Buffer) -> impl Iterator<Item = &[Cell]> {
    buf.content.chunks(usize::from(buf.area.width.max(1)))
}

fn same_style(a: &Cell, b: &Cell) -> bool {
    (a.fg, a.bg, a.modifier, a.underline_color) == (b.fg, b.bg, b.modifier, b.underline_color)
}

/// Returns the SGR codes of a cell's style, such as `38;5;40;48;5;235`.
fn codes(cell: &Cell) -> String {
    let mut codes = Vec::new();
    codes.extend(color(38, cell.fg));
    if cell.modifier.contains(Modifier::BOLD) {
        codes.push("1".to_owned());
    }
    codes.extend(color(48, cell.bg));
    if cell.modifier.contains(Modifier::UNDERLINED) {
        codes.push("4".to_owned());
        codes.extend(color(58, cell.underline_color));
    }
    codes.join(";")
}

fn color(code: u8, color: Color) -> Option<String> {
    match color {
        Color::Indexed(index) => Some(format!("{code};5;{index}")),
        Color::Rgb(red, green, blue) => Some(format!("{code};2;{red};{green};{blue}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    use super::*;

    #[test]
    fn writes_each_run_of_one_style_once() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        let green = Style::new().fg(Color::Indexed(40)).bg(Color::Indexed(235));
        let name = Style::new()
            .fg(Color::Indexed(255))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            .underline_color(Color::Indexed(244));
        buf.set_string(0, 0, "ab", green);
        buf.set_string(3, 0, "n", name);
        assert_eq!(plain(&buf), ["ab n "]);
        assert_eq!(
            ansi(&buf),
            ["\x1b[38;5;40;48;5;235mab\x1b[0m \x1b[38;5;255;1;4;58;5;244mn\x1b[0m "]
        );
    }
}
