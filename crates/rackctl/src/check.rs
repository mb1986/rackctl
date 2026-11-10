//! `rackctl check`: loads the catalog and the configuration, reports every problem and
//! summarizes each rack.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use rackctl_core::catalog::{Catalog, Kind, Origin};
use rackctl_core::config::{self, Config};
use rackctl_core::kdl_reader::FileError;
use rackctl_core::rack::{Placement, Rack};
use rackctl_core::wiring::{Cabling, LinkKind};
use textwrap::core::Word;
use textwrap::{Options, WordSeparator, WordSplitter};

use crate::paths::Locations;
use crate::style::{ERROR, HEADING, LABEL, NOTE, OK};
use crate::{CONFIG_ERROR, find_config, open_catalog, write_reports};

/// The width that long lines of the summary are wrapped to.
const WIDTH: usize = 80;

/// The width of the label column.
const LABEL_WIDTH: usize = 9;

/// Runs the command. `config` is the configuration named with `-c`, if any.
pub fn run(config: Option<PathBuf>, locations: &Locations) -> io::Result<ExitCode> {
    let mut err = io::stderr();
    let Some(path) = find_config(config, locations, &mut err)? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let Some(catalog) = open_catalog(Some(&path), locations, &mut err)? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };

    let mut out = anstream::stdout();
    let invalid_models = catalog.invalid_models();
    write_catalog(&mut out, &catalog, invalid_models.len())?;

    let loaded = config::load(&path, &catalog);
    row(&mut out, "", "config", &format!("{}: {}", locations.display(&path), status(&loaded)))?;
    let errors = match loaded {
        Ok(config) => {
            write_wiring(&mut out, config.cabling.as_ref())?;
            if invalid_models.is_empty() {
                for rack in config.layout.racks() {
                    writeln!(out)?;
                    write_summary(&mut out, rack, &catalog)?;
                }
                return Ok(ExitCode::SUCCESS);
            }
            Vec::new()
        }
        Err(errors) => errors,
    };
    out.flush()?;

    write_reports(&mut err, errors.iter().chain(invalid_models))?;
    Ok(ExitCode::from(CONFIG_ERROR))
}

/// Describes the result of loading the configuration: `ok` with the number of files read,
/// or the number of problems and of the files that have them.
fn status(loaded: &Result<Config, Vec<FileError>>) -> String {
    match loaded {
        Ok(config) => format!("{OK}ok{OK:#}, {}", plural(config.files.len(), "file", "files")),
        Err(errors) => {
            let problems = errors.iter().map(|error| error.problems().len()).sum();
            let problems = plural(problems, "problem", "problems");
            let files = plural(errors.len(), "file", "files");
            format!("{ERROR}{problems} in {files}{ERROR:#}")
        }
    }
}

/// Writes the wiring line: the cables of each type, or that there is no wiring.
fn write_wiring(out: &mut impl Write, cabling: Option<&Cabling>) -> io::Result<()> {
    let Some(cabling) = cabling else {
        return row(out, "", "wiring", &format!("{NOTE}none{NOTE:#}"));
    };
    let links = cabling.links();
    let kinds: Vec<String> = [LinkKind::Power, LinkKind::Net, LinkKind::Mgmt]
        .into_iter()
        .map(|kind| (kind, links.iter().filter(|link| link.kind == kind).count()))
        .filter(|&(_, count)| count > 0)
        .map(|(kind, count)| format!("{count} {}", <&str>::from(kind)))
        .collect();
    let cables = plural(links.len(), "cable", "cables");
    let kinds = if kinds.is_empty() {
        String::new()
    } else {
        format!(" {NOTE}({}){NOTE:#}", kinds.join(", "))
    };
    row(out, "", "wiring", &format!("{cables}{kinds}"))
}

fn write_catalog(out: &mut impl Write, catalog: &Catalog, invalid: usize) -> io::Result<()> {
    let total = catalog.ids().count();
    let user = catalog.ids().filter(|id| catalog.origin(id) == Some(Origin::User)).count();
    let validity = if invalid == 0 {
        format!("{OK}all valid{OK:#}")
    } else {
        format!("{ERROR}{invalid} with problems{ERROR:#}")
    };
    let origins = format!("{NOTE}({} built-in, {user} user){NOTE:#}", total - user);
    row(out, "", "catalog", &format!("{}, {validity} {origins}", plural(total, "model", "models")))
}

/// Writes the summary of a valid rack.
fn write_summary(out: &mut impl Write, rack: &Rack, catalog: &Catalog) -> io::Result<()> {
    writeln!(out, "{HEADING}rack \"{}\", {}U{HEADING:#}", rack.name, rack.units)?;

    let mut kinds = BTreeMap::<Kind, usize>::new();
    for device in &rack.devices {
        if let Ok(model) = catalog.model(&device.model) {
            *kinds.entry(model.kind).or_default() += 1;
        }
    }
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort_by_key(|&(_, count)| Reverse(count));
    let kinds: Vec<_> = kinds
        .into_iter()
        .map(|(kind, count)| format!("{count} {}", kind_name(kind, count)))
        .collect();
    let count = rack.devices.len();
    let lead = if count == 0 { "0".to_owned() } else { format!("{count}:") };
    list(out, "devices", &lead, &kinds)?;

    let models = rack.devices.iter().map(|device| device.model.as_str()).collect::<BTreeSet<_>>();
    row(out, "  ", "models", &plural(models.len(), "different model", "different models"))?;

    let free = rack.free_units(catalog);
    let free_count: u16 = free.iter().map(|units| units.count()).sum();
    let used = u16::from(rack.units) - free_count;
    row(out, "  ", "space", &format!("{used} of {} U used, {free_count} U free", rack.units))?;
    if free.is_empty() {
        row(out, "  ", "free", "none")?;
    } else {
        list(out, "free", "", &free.iter().map(ToString::to_string).collect::<Vec<_>>())?;
    }

    let strips: Vec<_> = rack
        .devices
        .iter()
        .filter_map(|device| {
            let Placement::Strip { side, .. } = device.placement else { return None };
            let model = catalog.model(&device.model).ok()?;
            let units = device.units(model, rack.units);
            let (side, face): (&str, &str) = (side.into(), device.face.into());
            Some(format!("{} ({side}, {face}, {units})", device.id))
        })
        .collect();
    if !strips.is_empty() {
        list(out, "strips", "", &strips)?;
    }
    Ok(())
}

/// Writes one labelled line.
fn row(out: &mut impl Write, indent: &str, label: &str, value: &str) -> io::Result<()> {
    writeln!(out, "{indent}{LABEL}{label:<LABEL_WIDTH$}{LABEL:#}{value}")
}

/// Writes a labelled, comma-separated list after `lead`, wrapped to [`WIDTH`] between items.
fn list(out: &mut impl Write, label: &str, lead: &str, items: &[String]) -> io::Result<()> {
    let value = format!("{lead} {}", items.join(", "));
    let line = format!("  {LABEL}{label:<LABEL_WIDTH$}{LABEL:#}{}", value.trim());
    let indent = " ".repeat(2 + LABEL_WIDTH);
    let options = Options::new(WIDTH)
        .word_separator(WordSeparator::Custom(after_commas))
        .word_splitter(WordSplitter::NoHyphenation)
        .break_words(false)
        .subsequent_indent(&indent);
    for line in textwrap::wrap(&line, options) {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// Splits a line into words only after `", "`, so that list items are never broken.
fn after_commas(line: &str) -> Box<dyn Iterator<Item = Word<'_>> + '_> {
    Box::new(line.split_inclusive(", ").map(Word::from))
}

/// Names a kind of device for a count of them, for example `2 switches`.
const fn kind_name(kind: Kind, count: usize) -> &'static str {
    let (one, many) = match kind {
        Kind::Server => ("server", "servers"),
        Kind::Storage => ("storage", "storage"),
        Kind::Tape => ("tape drive", "tape drives"),
        Kind::Kvm => ("KVM", "KVMs"),
        Kind::Switch => ("switch", "switches"),
        Kind::Router => ("router", "routers"),
        Kind::PatchPanel => ("patch panel", "patch panels"),
        Kind::Pdu => ("PDU", "PDUs"),
        Kind::Ups => ("UPS", "UPSs"),
        Kind::Blank => ("blank panel", "blank panels"),
        Kind::Shelf => ("shelf", "shelves"),
        Kind::Generic => ("other", "other"),
    };
    if count == 1 { one } else { many }
}

/// Formats a count with the singular or plural noun, for example `1 model` or `3 models`.
fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a list and returns it without styles.
    fn plain_list(label: &str, lead: &str, items: &[String]) -> String {
        let mut out = Vec::new();
        list(&mut out, label, lead, items).expect("written");
        let text = String::from_utf8(out).expect("UTF-8");
        anstream::adapter::strip_str(&text).to_string()
    }

    #[test]
    fn wraps_lists_between_items() {
        let items: Vec<_> = (1..=8).map(|n| format!("item number {n}")).collect();
        assert_eq!(
            plain_list("things", "8:", &items).lines().collect::<Vec<_>>(),
            [
                "  things   8: item number 1, item number 2, item number 3, item number 4,",
                "           item number 5, item number 6, item number 7, item number 8",
            ]
        );
    }

    #[test]
    fn writes_a_lead_or_items_alone() {
        assert_eq!(plain_list("devices", "0", &[]), "  devices  0\n");
        assert_eq!(plain_list("free", "", &["U1-U4".to_owned()]), "  free     U1-U4\n");
    }

    #[test]
    fn names_kinds_and_counts() {
        assert_eq!(kind_name(Kind::Switch, 2), "switches");
        assert_eq!(kind_name(Kind::Ups, 1), "UPS");
        assert_eq!(plural(1, "model", "models"), "1 model");
        assert_eq!(plural(3, "problem", "problems"), "3 problems");
    }
}
