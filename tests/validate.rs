//! Integration tests for the `validate` subcommand logic.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn unique_tmp(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("roadmark-test-{label}-{pid}-{n}"))
}

/// A config for the inline trees below. `type` and `area` are mandatory
/// frontmatter, so every tree carries those axes and — since #34 —
/// every config must declare them; `horizon` is declared only where a
/// feature actually carries one.
const CONFIG_WITH_HORIZON: &str = "versions = [\"v0.2.x\"]\n\
     [fields.type]\nvalues = [\"feature\", \"fix\", \"chore\"]\n\
     [fields.area]\nvalues = [\"x\"]\nmulti = true\n\
     [fields.horizon]\nvalues = [\"next\"]\n";

const CONFIG_WITHOUT_HORIZON: &str = "versions = [\"v0.2.x\"]\n\
     [fields.type]\nvalues = [\"feature\", \"fix\", \"chore\"]\n\
     [fields.area]\nvalues = [\"x\"]\nmulti = true\n";

/// One feature file: `+++` frontmatter with the given id, plus a body.
fn feature_src(id: &str, extra_frontmatter: &str, body: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\ntype = \"feature\"\narea = [\"x\"]\n\
         {extra_frontmatter}status = \"todo\"\ntarget = [\"v0.2.x\"]\n+++\n\n{body}"
    )
}

fn render_minimal() -> String {
    let root = fixture("minimal");
    let config = roadmark::load_config(&root).unwrap();
    let mut features = roadmark::load_features(&root).unwrap();
    roadmark::sort_features(&mut features, &config);
    roadmark::render(&features, &config)
}

#[test]
fn clean_run_against_matching_roadmap() {
    let root = fixture("minimal");
    let tmp = unique_tmp("clean");
    std::fs::create_dir_all(&tmp).unwrap();
    let roadmap_md = tmp.join("ROADMAP.md");
    std::fs::write(&roadmap_md, render_minimal()).unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert!(
        report.is_clean(),
        "expected clean, got:\n{}",
        report.to_text()
    );
}

#[test]
fn drift_when_roadmap_lacks_an_anchor() {
    let root = fixture("minimal");
    let tmp = unique_tmp("missing-anchor");
    std::fs::create_dir_all(&tmp).unwrap();
    let roadmap_md = tmp.join("ROADMAP.md");
    // Write a stub that contains only one of the fixture's anchors.
    std::fs::write(&roadmap_md, r#"<a id="f22"></a>"#).unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert!(report.has_drift());
    assert!(!report.has_hard_errors());
    assert!(report
        .anchors_missing_from_disk
        .contains(&"f-llm-plugin".to_string()));
    assert!(report
        .anchors_missing_from_disk
        .contains(&"f-roadmap-toml-source".to_string()));
}

#[test]
fn drift_when_roadmap_has_orphan_anchor() {
    let root = fixture("minimal");
    let tmp = unique_tmp("orphan-anchor");
    std::fs::create_dir_all(&tmp).unwrap();
    let roadmap_md = tmp.join("ROADMAP.md");
    let mut content = render_minimal();
    content.push_str("\n<a id=\"f-deleted-feature\"></a>\n");
    std::fs::write(&roadmap_md, content).unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert!(report.has_drift());
    assert_eq!(
        report.anchors_missing_from_regen,
        vec!["f-deleted-feature".to_string()]
    );
    assert!(report.anchors_missing_from_disk.is_empty());
}

#[test]
fn schema_error_does_not_abort_run() {
    // Build a temp .roadmap/ with one valid + one broken feature file.
    let root = unique_tmp("schema-err");
    let features = root.join("features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(root.join("config.toml"), CONFIG_WITH_HORIZON).unwrap();
    std::fs::write(
        features.join("f-good.md"),
        "+++\nid = \"F-good\"\ntype = \"feature\"\narea = [\"x\"]\nhorizon = \"next\"\nstatus = \"todo\"\ntarget = [\"v0.2.x\"]\n+++\n\nGood.\n",
    )
    .unwrap();
    std::fs::write(features.join("f-bad.md"), "no fence here\n").unwrap();

    let tmp_md = unique_tmp("schema-err-md");
    std::fs::create_dir_all(&tmp_md).unwrap();
    let roadmap_md = tmp_md.join("ROADMAP.md");
    std::fs::write(&roadmap_md, "<a id=\"f-good\"></a>\n").unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert_eq!(report.schema_errors.len(), 1, "{:?}", report.schema_errors);
    assert!(report.schema_errors[0].path.ends_with("f-bad.md"));
}

/// A board-canonical project: no feature carries a horizon, and the config
/// therefore declares none. Before #34 the missing `[fields.horizon]` was a
/// hard error, so the exact tree ADR-0002 unblocked could not pass the gate.
#[test]
fn feature_without_horizon_validates_clean() {
    let root = unique_tmp("no-horizon");
    let features = root.join("features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(root.join("config.toml"), CONFIG_WITHOUT_HORIZON).unwrap();
    // No `horizon` key at all — priority lives on an external board.
    std::fs::write(
        features.join("f-board.md"),
        feature_src("F-board", "", "Board-driven.\n"),
    )
    .unwrap();

    let config = roadmark::load_config(&root).unwrap();
    let mut fs = roadmark::load_features(&root).unwrap();
    roadmark::sort_features(&mut fs, &config);
    let rendered = roadmark::render(&fs, &config);
    // No feature carries a horizon → the column is omitted outright.
    assert!(
        !rendered.contains("Horizon"),
        "horizon column should be omitted:\n{rendered}"
    );

    let tmp_md = unique_tmp("no-horizon-md");
    std::fs::create_dir_all(&tmp_md).unwrap();
    let roadmap_md = tmp_md.join("ROADMAP.md");
    std::fs::write(&roadmap_md, &rendered).unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert!(
        report.is_clean(),
        "expected clean, got:\n{}",
        report.to_text()
    );
}

#[test]
fn anchor_collision_detected() {
    let root = unique_tmp("collision");
    let features = root.join("features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(root.join("config.toml"), CONFIG_WITH_HORIZON).unwrap();
    // Two distinct IDs that lowercase to the same anchor.
    std::fs::write(
        features.join("f-foo-1.md"),
        "+++\nid = \"F-Foo\"\ntype = \"feature\"\narea = [\"x\"]\nhorizon = \"next\"\nstatus = \"todo\"\ntarget = [\"v0.2.x\"]\n+++\n\nA.\n",
    )
    .unwrap();
    std::fs::write(
        features.join("f-foo-2.md"),
        "+++\nid = \"f-foo\"\ntype = \"feature\"\narea = [\"x\"]\nhorizon = \"next\"\nstatus = \"todo\"\ntarget = [\"v0.2.x\"]\n+++\n\nB.\n",
    )
    .unwrap();

    let tmp_md = unique_tmp("collision-md");
    std::fs::create_dir_all(&tmp_md).unwrap();
    let roadmap_md = tmp_md.join("ROADMAP.md");
    std::fs::write(&roadmap_md, "").unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert_eq!(report.anchor_collisions.len(), 1);
    assert_eq!(report.anchor_collisions[0].anchor, "f-foo");
}

/// A tree carrying a horizon still owes `[fields.horizon]`: the declared
/// value order is what ranks the features that carry one (#34).
#[test]
fn horizon_in_use_without_a_declaration_is_a_hard_error() {
    let root = unique_tmp("horizon-undeclared");
    let features = root.join("features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(root.join("config.toml"), CONFIG_WITHOUT_HORIZON).unwrap();
    std::fs::write(
        features.join("f-ranked.md"),
        feature_src("F-ranked", "horizon = \"next\"\n", "Ranked.\n"),
    )
    .unwrap();

    let tmp_md = unique_tmp("horizon-undeclared-md");
    std::fs::create_dir_all(&tmp_md).unwrap();
    let roadmap_md = tmp_md.join("ROADMAP.md");
    std::fs::write(&roadmap_md, "").unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert!(report.has_hard_errors());
    assert!(
        report
            .schema_errors
            .iter()
            .any(|e| e.message.contains("missing `[fields.horizon]`")),
        "got: {}",
        report.to_text()
    );
}

/// End-to-end #38 + #36 + the warnings tier: an unwritten body and a prose
/// mention of a missing id are both reported, and neither fails the run.
#[test]
fn empty_body_and_dead_prose_reference_warn_without_failing() {
    let root = unique_tmp("warnings");
    let features = root.join("features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(root.join("config.toml"), CONFIG_WITHOUT_HORIZON).unwrap();
    // Freshly scaffolded: frontmatter written, body still empty.
    std::fs::write(
        features.join("f-blank.md"),
        feature_src("F-blank", "", "   \n"),
    )
    .unwrap();
    std::fs::write(
        features.join("f-prose.md"),
        feature_src("F-prose", "", "Sibling to `F-gone`, roughly.\n"),
    )
    .unwrap();

    let config = roadmark::load_config(&root).unwrap();
    let mut fs = roadmark::load_features(&root).unwrap();
    roadmark::sort_features(&mut fs, &config);
    let tmp_md = unique_tmp("warnings-md");
    std::fs::create_dir_all(&tmp_md).unwrap();
    let roadmap_md = tmp_md.join("ROADMAP.md");
    std::fs::write(&roadmap_md, roadmark::render(&fs, &config)).unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert!(!report.has_hard_errors(), "got: {}", report.to_text());
    assert!(!report.has_drift(), "got: {}", report.to_text());
    assert_eq!(report.warnings.len(), 2, "got: {}", report.to_text());
    let text = report.to_text();
    assert!(text.contains("empty body"), "got: {text}");
    assert!(
        text.contains("reference to unknown feature id F-gone"),
        "got: {text}"
    );
    assert!(!text.contains("validate: clean"), "got: {text}");
}

/// The same missing id, written as a link, is a hard error: the generated
/// roadmap would ship a dead anchor (#36).
#[test]
fn dead_link_reference_fails_the_run() {
    let root = unique_tmp("dead-link");
    let features = root.join("features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(root.join("config.toml"), CONFIG_WITHOUT_HORIZON).unwrap();
    std::fs::write(
        features.join("f-here.md"),
        feature_src("F-here", "", "Successor of [F-gone](#f-gone).\n"),
    )
    .unwrap();

    let tmp_md = unique_tmp("dead-link-md");
    std::fs::create_dir_all(&tmp_md).unwrap();
    let roadmap_md = tmp_md.join("ROADMAP.md");
    std::fs::write(&roadmap_md, "").unwrap();

    let report = roadmark::validate::validate(&root, &roadmap_md, false).unwrap();
    assert_eq!(report.dangling_links.len(), 1, "got: {}", report.to_text());
    assert!(report.has_hard_errors());
    assert!(report.dangling_links[0].path.ends_with("f-here.md"));
}

/// Run the built binary so the clap wiring (`--root` provenance) is under
/// test, not just the library function.
fn run_validate(args: &[&std::ffi::OsStr]) -> (i32, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_roadmark"))
        .args(args)
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

/// #31: an explicitly passed `--root` that is not there fails, naming the
/// resolved path. Exit 0 would be a clean pass for a run that checked
/// nothing — the failure mode `validate` exists to prevent.
#[test]
fn cli_explicit_bad_root_fails_naming_the_path() {
    let missing = unique_tmp("bad-root").join("nope");
    let (code, text) = run_validate(&["--root".as_ref(), missing.as_os_str(), "validate".as_ref()]);
    assert_eq!(code, 1, "got: {text}");
    assert!(text.contains(&missing.display().to_string()), "got: {text}");
}

/// …and an explicit `--root` at a real directory that simply has no
/// `features/` fails the same way: the user named a tree, it isn't one.
#[test]
fn cli_explicit_root_without_features_fails() {
    let dir = unique_tmp("root-no-features");
    std::fs::create_dir_all(&dir).unwrap();
    let (code, text) = run_validate(&["--root".as_ref(), dir.as_os_str(), "validate".as_ref()]);
    assert_eq!(code, 1, "got: {text}");
    assert!(text.contains(&dir.display().to_string()), "got: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression guard for the guarantee AGENTS.md names as one not to break:
/// with `--root` defaulted and no `.roadmap/` on the checkout, `validate`
/// silent-passes so the same CI recipe runs everywhere.
#[test]
fn cli_default_root_without_source_still_exits_zero() {
    let dir = unique_tmp("default-root-skip");
    std::fs::create_dir_all(&dir).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_roadmark"))
        .arg("validate")
        .current_dir(&dir)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "got: {text}");
    assert!(text.contains("skipped"), "got: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}
