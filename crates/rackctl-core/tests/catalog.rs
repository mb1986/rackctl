//! Checks the built-in catalog shipped in the repository.

use std::path::Path;

use miette::{GraphicalReportHandler, GraphicalTheme};
use rackctl_core::catalog::{Catalog, ModelError};

#[test]
fn every_built_in_model_loads() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    let catalog = Catalog::open(&[&dir]).expect("readable catalog directory");
    assert!(catalog.ids().next().is_some(), "no models found in {}", dir.display());

    let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());
    let mut report = String::new();
    for id in catalog.ids() {
        if let Err(ModelError::Invalid(error)) = catalog.model(id) {
            handler.render_report(&mut report, error).expect("rendered report");
        }
    }
    assert!(report.is_empty(), "invalid models in the built-in catalog:\n{report}");
}
