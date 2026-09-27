//! Checks the built-in catalog shipped in the repository.

use std::path::Path;

use miette::{GraphicalReportHandler, GraphicalTheme};
use rackctl_core::catalog::Catalog;

#[test]
fn embeds_every_model_in_the_catalog_folder() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    let on_disk = Catalog::open(&[&dir]).expect("readable catalog directory");
    assert!(on_disk.ids().next().is_some(), "no models found in {}", dir.display());
    assert!(Catalog::builtin().ids().eq(on_disk.ids()));
}

#[test]
fn every_built_in_model_loads() {
    let catalog = Catalog::builtin();
    let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());
    let mut output = String::new();
    for report in catalog.invalid_models().iter().flat_map(|error| error.reports()) {
        handler.render_report(&mut output, report.as_ref()).expect("rendered report");
    }
    assert!(output.is_empty(), "invalid models in the built-in catalog:\n{output}");
}
