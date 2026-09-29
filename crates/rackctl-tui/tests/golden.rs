//! Draws the catalog models as the golden files show them.

use std::fs;
use std::path::PathBuf;

use rackctl_core::catalog::Catalog;
use rackctl_tui::ui::art::Sample;
use rackctl_tui::ui::preview::preview;
use rackctl_tui::ui::text;

#[test]
fn draws_every_face_as_its_golden_files() {
    let root: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../../tests/golden/faces"].iter().collect();
    let catalog = Catalog::builtin();
    let (mut checked, mut different) = (0, Vec::new());
    for vendor in fs::read_dir(&root).expect("golden faces") {
        let vendor = vendor.expect("vendor directory").path();
        for file in fs::read_dir(&vendor).expect("vendor directory") {
            let path = file.expect("golden file").path();
            let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            // Strip faces are drawn beside the rack, not in a slice of it.
            let Some(stem) = file_name.strip_suffix(".txt").filter(|stem| !stem.contains(".strip"))
            else {
                continue;
            };
            let (name, variant) = stem.split_once('.').unwrap_or((stem, ""));
            let vendor_name = vendor.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            let id = format!("{vendor_name}/{name}");
            let model = catalog.model(&id).unwrap_or_else(|error| panic!("{id}: {error}"));
            let face = model.faces.normal.as_ref().unwrap_or_else(|| panic!("{id}: no face"));
            let sample = if variant == "off" { Sample::Off } else { Sample::Normal };
            let buf = preview(model, face, sample, variant == "numbers", "srv01");
            let drawn: String = text::plain(&buf).into_iter().map(|line| line + "\n").collect();
            if drawn != fs::read_to_string(&path).expect("golden file") {
                different.push(format!("{vendor_name}/{file_name}"));
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "no golden files in {}", root.display());
    assert!(different.is_empty(), "drawn differently: {different:?}");
}
