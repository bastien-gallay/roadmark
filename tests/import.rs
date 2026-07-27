//! Integration tests for `import`: the whole point is that the imported
//! tree *runs*, so these drive the real pipeline rather than inspecting
//! strings — `import` → `generate` → `validate`, end to end.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("roadmark-import-{label}-{pid}-{n}"))
}

/// A hand-written roadmap in the shape the issue described: bucket
/// headings, a decorated status glyph, a multi-valued column under a
/// non-obvious header, and prose between the tables.
const SOURCE: &str = "\
# faceto — Roadmap

Some preamble about where the project is going.

## Must

| ID | Direction | Status | Horizon | Summary |
| --- | --- | --- | --- | --- |
| F-region-frontiers | model · grouping | ✅ | ✅ Shipped | Regions are a contiguous partition. |
| F-seam-clusters | model | 🚧 | **Now** | Code surfaces several features share. |

### Strategic review 2026-07-06

Three-horizons pass.

## Should

| ID | Direction | Status | Horizon | Summary |
| --- | --- | --- | --- | --- |
| F-llm-plugin | adoption | ☐ | Later | Out-of-band suggestion stream. |
";

fn write_source(root: &PathBuf, body: &str) -> PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let path = root.join("ROADMAP.md");
    std::fs::write(&path, body).unwrap();
    path
}

fn options(maps: &[&str]) -> roadmark::import::ImportOptions {
    let mut options = roadmark::import::ImportOptions::default();
    for spec in maps {
        options.add_mapping(spec).unwrap();
    }
    options
}

/// The claim that matters: after an import, `generate` runs and
/// `validate` does not *fail*. An imported tree that crashes the very next
/// command would be worse than no import at all.
#[test]
fn an_imported_tree_generates_and_validates() {
    let dir = unique_tmp("pipeline");
    let source = write_source(&dir, SOURCE);
    let root = dir.join(".roadmap");

    let outcome =
        roadmark::import::import(&root, &source, &options(&["area=Direction"]), false).unwrap();
    assert_eq!(outcome.created.len(), 3, "{:?}", outcome.created);
    assert!(outcome.config_written.is_some());
    assert!(outcome.leftovers_written.is_some());

    let config = roadmark::load_config(&root).unwrap();
    let mut features = roadmark::load_features(&root, &config).unwrap();
    roadmark::sort_features(&mut features, &config);
    let rendered = roadmark::render(&features, &config, &[]);
    let roadmap_md = dir.join("OUT.md");
    std::fs::write(&roadmap_md, &rendered).unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, true).unwrap();
    assert!(!report.has_hard_errors(), "got:\n{}", report.to_text());

    // Buckets came from the headings, so `versions` and `target` agree and
    // the rows land in document order rather than all in one tail.
    assert_eq!(config.versions, vec!["Must", "Should"]);
    assert!(rendered.contains("| Must |"), "got:\n{rendered}");
    // A decorated horizon cell keeps only its value…
    assert!(rendered.contains("| shipped |"), "got:\n{rendered}");
    // …and a `·`-separated column becomes a multi-valued axis.
    assert!(rendered.contains("model, grouping"), "got:\n{rendered}");
    // Glyphs round-trip: ✅ in, ✅ out.
    assert!(rendered.contains("| ✅ |"), "got:\n{rendered}");
}

/// The other shape a hand-written roadmap takes, and arguably the more
/// common one: checkbox bullets under bucket headings, zero tables (#57).
/// Same claim as above — the imported tree generates and validates.
#[test]
fn a_checkbox_bullet_roadmap_imports_generates_and_validates() {
    let dir = unique_tmp("bullets");
    let source = write_source(
        &dir,
        "# termherd — Roadmap\n\n\
         Some preamble.\n\n\
         ## Must\n\n\
         - [x] `F-app-shell` — window, lifecycle, bounds (menu: deferred to M3 with\n  \
         the keymap — no native menu API in iced). The deferral left one visible gap\n  \
         on macOS: winit builds only the *application* menu.\n\
         - [ ] `F-packaging-ci` — signed mac/win/linux builds + CI gate\n  \
         - [ ] notarisation still open\n\n\
         ## Should\n\n\
         - [~] `F-keymap` — user-rebindable keys.\n",
    );
    let root = dir.join(".roadmap");

    let outcome = roadmark::import::import(&root, &source, &options(&[]), false).unwrap();
    assert_eq!(outcome.created.len(), 3, "{:?}", outcome.created);

    let config = roadmark::load_config(&root).unwrap();
    assert_eq!(config.versions, vec!["Must", "Should"]);
    let mut features = roadmark::load_features(&root, &config).unwrap();
    roadmark::sort_features(&mut features, &config);
    let rendered = roadmark::render(&features, &config, &[]);
    let roadmap_md = dir.join("OUT.md");
    std::fs::write(&roadmap_md, &rendered).unwrap();
    let report = roadmark::validate::validate(&root, &roadmap_md, true).unwrap();
    assert!(!report.has_hard_errors(), "got:\n{}", report.to_text());

    // The checkbox is the status and the heading is the bucket.
    assert!(rendered.contains("| ✅ |"), "got:\n{rendered}");
    assert!(rendered.contains("| 🚧 |"), "got:\n{rendered}");
    // The catalog cell is the first sentence, not the whole entry: the
    // second sentence is in the body but must not reach the row.
    assert!(
        rendered.contains("no native menu API in iced)."),
        "got:\n{rendered}"
    );
    let catalog = rendered.split("## Details").next().unwrap();
    assert!(
        !catalog.contains("The deferral left"),
        "second sentence leaked into the catalog:\n{catalog}"
    );
    // …and the details keep everything, nested bullet included.
    assert!(rendered.contains("The deferral left"), "got:\n{rendered}");
    assert!(
        rendered.contains("- [ ] notarisation still open"),
        "got:\n{rendered}"
    );
}

/// The undecidable axes are commented out, and the mandatory ones carry a
/// placeholder rather than a comment — commenting `type`/`area`/`target`
/// would produce a file that doesn't parse, so `generate` would fail
/// before the adopter ever saw their roadmap.
#[test]
fn mandatory_fields_get_placeholders_and_optional_ones_get_comments() {
    let dir = unique_tmp("shape");
    let source = write_source(
        &dir,
        "| ID | Status | Summary |\n| --- | --- |--- |\n| F-a | ☐ | A thing. |\n",
    );
    let root = dir.join(".roadmap");
    roadmark::import::import(&root, &source, &options(&[]), false).unwrap();

    let written = std::fs::read_to_string(root.join("features/f-a.md")).unwrap();
    assert!(written.contains("type = \"feature\""), "got:\n{written}");
    assert!(written.contains("area = [\"<TODO>\"]"), "got:\n{written}");
    assert!(written.contains("target = [\"<TODO>\"]"), "got:\n{written}");
    assert!(written.contains("# class ="), "got:\n{written}");
    assert!(written.contains("# effort ="), "got:\n{written}");
    // It parses — which is the whole reason those three aren't comments.
    let config = roadmark::load_config(&root).unwrap();
    assert!(roadmark::load_features(&root, &config).is_ok());

    // …and `validate` names every placeholder, as a warning: scaffolding
    // first and deciding second is the shape of adoption, not an error.
    let roadmap_md = dir.join("OUT.md");
    std::fs::write(&roadmap_md, "").unwrap();
    let report = roadmark::validate::validate(&root, &roadmap_md, true).unwrap();
    assert!(!report.has_hard_errors(), "got:\n{}", report.to_text());
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|w| w.message.contains("scaffolded placeholder"))
            .count(),
        2,
        "got:\n{}",
        report.to_text()
    );
}

#[test]
fn dry_run_writes_nothing_but_reports_everything() {
    let dir = unique_tmp("dry");
    let source = write_source(&dir, SOURCE);
    let root = dir.join(".roadmap");

    let outcome =
        roadmark::import::import(&root, &source, &options(&["area=Direction"]), true).unwrap();
    assert!(outcome.dry_run);
    assert_eq!(outcome.created.len(), 3);
    assert!(outcome.config_written.is_some());
    assert!(!root.exists(), "dry run created {}", root.display());
}

/// A re-run must not clobber edits made since the first import — the
/// commented-out fields are exactly what a human goes and fills in.
#[test]
fn a_second_import_skips_files_that_already_exist() {
    let dir = unique_tmp("rerun");
    let source = write_source(&dir, SOURCE);
    let root = dir.join(".roadmap");
    roadmark::import::import(&root, &source, &options(&[]), false).unwrap();

    let edited = root.join("features/f-seam-clusters.md");
    let mine = std::fs::read_to_string(&edited)
        .unwrap()
        .replace("# effort = \"M\"", "effort = \"L\"");
    std::fs::write(&edited, &mine).unwrap();

    let second = roadmark::import::import(&root, &source, &options(&[]), false).unwrap();
    assert!(second.created.is_empty(), "{:?}", second.created);
    assert_eq!(second.skipped.len(), 3);
    assert_eq!(std::fs::read_to_string(&edited).unwrap(), mine);
}

/// Prose between tables is the reasoning a roadmap is read for. Dropping
/// it silently would be the one unrecoverable thing an import could do.
#[test]
fn unattributable_prose_is_kept() {
    let dir = unique_tmp("leftovers");
    let source = write_source(&dir, SOURCE);
    let root = dir.join(".roadmap");
    roadmark::import::import(&root, &source, &options(&[]), false).unwrap();

    let leftovers = std::fs::read_to_string(root.join("import-leftovers.md")).unwrap();
    assert!(leftovers.contains("Some preamble"), "got:\n{leftovers}");
    assert!(
        leftovers.contains("Strategic review 2026-07-06"),
        "got:\n{leftovers}"
    );
    assert!(
        leftovers.contains("Three-horizons pass."),
        "got:\n{leftovers}"
    );
    // The rows themselves moved into feature files, not here.
    assert!(
        !leftovers.contains("F-region-frontiers"),
        "got:\n{leftovers}"
    );
}

#[test]
fn a_source_with_no_features_fails_with_a_usable_message() {
    let dir = unique_tmp("no-table");
    let source = write_source(&dir, "# Roadmap\n\nJust prose, no table.\n");
    let root = dir.join(".roadmap");
    let err = roadmark::import::import(&root, &source, &options(&[]), true).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no importable features"), "got: {msg}");
    assert!(msg.contains("--map"), "got: {msg}");
    // Both readable shapes are named, so the message says what to write.
    assert!(msg.contains("checkbox bullets"), "got: {msg}");
}
