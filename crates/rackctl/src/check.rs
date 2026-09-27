//! `rackctl check`: loads the catalog and the rack file, reports every problem and
//! summarizes the rack.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::process::ExitCode;

use rackctl_core::catalog::{Catalog, Kind, ModelError, Origin};
use rackctl_core::config;
use rackctl_core::kdl_reader::FileError;
use rackctl_core::rack::{Placement, Rack};

use crate::CONFIG_ERROR;
use crate::paths::{self, Locations};
use crate::style::{ERROR, HEADING, LABEL, NOTE, OK};

/// The width that long lines of the summary are wrapped to.
const WIDTH: usize = 80;

/// The width of the label column.
const LABEL_WIDTH: usize = 9;

/// Runs the command. `config` is the rack file named with `-c` or `$RACKCTL_CONFIG`.
pub fn run(config: Option<PathBuf>, locations: &Locations) -> ExitCode {
    check(config, locations).unwrap_or(ExitCode::FAILURE)
}

fn check(config: Option<PathBuf>, locations: &Locations) -> io::Result<ExitCode> {
    let Some(rack_file) = config.or_else(|| locations.rack_file()) else {
        eprintln!("rackctl: cannot find the configuration because $HOME is not set; use -c FILE");
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let mut catalog = Catalog::builtin();
    let user_dir = paths::user_catalog(&rack_file);
    if let Err(error) = catalog.add_dirs(&[&user_dir]) {
        eprintln!("rackctl: cannot read {}: {error}", locations.display(&user_dir));
        return Ok(ExitCode::from(CONFIG_ERROR));
    }

    let mut out = anstream::stdout();
    let invalid_models = invalid_models(&catalog);
    write_catalog(&mut out, &catalog, invalid_models.len())?;

    let rack_name = locations.display(&rack_file);
    let mut failed = match config::load_rack(&rack_file, &catalog) {
        Ok(rack) => {
            row(&mut out, "", "rack", &format!("{rack_name}: {OK}ok{OK:#}"))?;
            if invalid_models.is_empty() {
                writeln!(out)?;
                write_summary(&mut out, &rack, &catalog)?;
                return Ok(ExitCode::SUCCESS);
            }
            Vec::new()
        }
        Err(errors) => {
            // The rack file comes first; the invalid models that follow are reported with
            // the catalog.
            let rack_error = errors.into_iter().next().expect("the rack file's problems");
            let count = problems(rack_error.problems().len());
            row(&mut out, "", "rack", &format!("{rack_name}: {ERROR}{count}{ERROR:#}"))?;
            vec![rack_error]
        }
    };
    out.flush()?;

    failed.extend(invalid_models);
    for report in failed.iter().flat_map(FileError::reports) {
        eprint!("\n{report:?}");
    }
    Ok(ExitCode::from(CONFIG_ERROR))
}

/// Loads every model in the catalog and returns the problems of those that are invalid.
fn invalid_models(catalog: &Catalog) -> Vec<FileError> {
    catalog
        .ids()
        .filter_map(|id| match catalog.model(id) {
            Err(ModelError::Invalid(error)) => Some(error.clone()),
            _ => None,
        })
        .collect()
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
    list(out, "devices", &format!("{}:", rack.devices.len()), &kinds)?;

    let models = rack.devices.iter().map(|device| device.model.as_str()).collect::<BTreeSet<_>>();
    row(out, "  ", "models", &plural(models.len(), "different model", "different models"))?;

    let free = rack.free_units(catalog);
    let free_count: u16 = free.iter().map(|units| units.end() - units.start() + 1).sum();
    let used = u16::from(rack.units) - free_count;
    row(out, "  ", "space", &format!("{used} of {} U used, {free_count} U free", rack.units))?;
    if free.is_empty() {
        row(out, "  ", "free", "none")?;
    } else {
        list(out, "free", "", &free.iter().map(describe_units).collect::<Vec<_>>())?;
    }

    let strips: Vec<_> = rack
        .devices
        .iter()
        .filter_map(|device| {
            let Placement::Strip { side, .. } = device.placement else { return None };
            let model = catalog.model(&device.model).ok()?;
            let units = describe_units(&device.units(model, rack.units));
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

/// Writes a labelled, comma-separated list after `lead`, wrapped to [`WIDTH`].
fn list(out: &mut impl Write, label: &str, lead: &str, items: &[String]) -> io::Result<()> {
    let indent = 2 + LABEL_WIDTH;
    let mut lines = wrap(lead, items, WIDTH - indent).into_iter();
    row(out, "  ", label, &lines.next().unwrap_or_default())?;
    for line in lines {
        writeln!(out, "{:indent$}{line}", "")?;
    }
    Ok(())
}

/// Joins `lead` and `items` with commas into lines no longer than `width`, breaking only
/// between items.
fn wrap(lead: &str, items: &[String], width: usize) -> Vec<String> {
    let mut lines = vec![lead.to_owned()];
    for (index, item) in items.iter().enumerate() {
        let piece = if index + 1 < items.len() { format!("{item},") } else { item.clone() };
        let line = lines.last_mut().expect("at least one line");
        if line.is_empty() {
            line.push_str(&piece);
        } else if line.len() + 1 + piece.len() <= width {
            line.push(' ');
            line.push_str(&piece);
        } else {
            lines.push(piece);
        }
    }
    lines
}

/// Describes a range of units, for example `U2-U5` or `U7`.
fn describe_units(units: &RangeInclusive<u16>) -> String {
    if units.start() == units.end() {
        format!("U{}", units.start())
    } else {
        format!("U{}-U{}", units.start(), units.end())
    }
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

/// Formats a number of problems, for example `1 problem` or `3 problems`.
fn problems(count: usize) -> String {
    plural(count, "problem", "problems")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_lists_between_items() {
        let items: Vec<_> = ["alpha", "beta", "gamma"].map(String::from).into();
        assert_eq!(wrap("3:", &items, 80), ["3: alpha, beta, gamma"]);
        assert_eq!(wrap("3:", &items, 14), ["3: alpha,", "beta, gamma"]);
        assert_eq!(wrap("", &items, 12), ["alpha, beta,", "gamma"]);
    }

    #[test]
    fn names_kinds_and_counts() {
        assert_eq!(kind_name(Kind::Switch, 2), "switches");
        assert_eq!(kind_name(Kind::Ups, 1), "UPS");
        assert_eq!(plural(1, "model", "models"), "1 model");
        assert_eq!(problems(3), "3 problems");
        assert_eq!(describe_units(&(3..=9)), "U3-U9");
        assert_eq!(describe_units(&(35..=35)), "U35");
    }
}
