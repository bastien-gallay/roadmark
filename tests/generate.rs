//! Integration test: load fixture .roadmap/, generate, snapshot.

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

#[test]
fn minimal_fixture_round_trip() {
    let root = fixture("minimal");
    let config = roadmark::load_config(&root).unwrap();
    let mut features = roadmark::load_features(&root).unwrap();
    roadmark::sort_features(&mut features, &config);
    let out = roadmark::render(&features, &config);
    insta::assert_snapshot!(out);
}

/// `generate --output` must write the same bytes the stdout form emits.
#[test]
fn output_flag_writes_the_rendered_document() {
    let tmp = unique_tmp("output-flag");
    std::fs::create_dir_all(&tmp).unwrap();
    let target = tmp.join("ROADMAP.md");

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_roadmark"))
        .args(["--root".as_ref(), fixture("minimal").as_os_str()])
        .arg("generate")
        .arg("-o")
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success(), "generate -o failed: {status:?}");

    let root = fixture("minimal");
    let config = roadmark::load_config(&root).unwrap();
    let mut features = roadmark::load_features(&root).unwrap();
    roadmark::sort_features(&mut features, &config);
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        roadmark::render(&features, &config)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The regression #41 exists to prevent: a failing `generate` must leave the
/// committed roadmap exactly as it found it. The shell redirection form
/// cannot offer this — it truncates the file before roadmark ever runs.
#[test]
fn failing_generate_does_not_destroy_the_existing_output_file() {
    let tmp = unique_tmp("output-preserved");
    std::fs::create_dir_all(tmp.join("features")).unwrap();
    std::fs::write(
        tmp.join("config.toml"),
        "versions = [\"v1\"]\n[fields.horizon]\nvalues = [\"now\"]\n",
    )
    .unwrap();
    // An unknown frontmatter key: rejected since 0.6.0 (`deny_unknown_fields`),
    // which is precisely the upgrade that made this data loss fire.
    std::fs::write(
        tmp.join("features").join("f-a.md"),
        "+++\nid = \"F-a\"\ntype = \"feature\"\narea = [\"x\"]\n\
         status = \"todo\"\ntarget = [\"v1\"]\nowner = \"someone\"\n+++\n\nBody.\n",
    )
    .unwrap();

    let target = tmp.join("ROADMAP.md");
    let precious = "IMPORTANT EXISTING ROADMAP CONTENT\n";
    std::fs::write(&target, precious).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_roadmark"))
        .args(["--root".as_ref(), tmp.as_os_str()])
        .arg("generate")
        .arg("-o")
        .arg(&target)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected the unexpected-error code"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        precious,
        "a failed generate overwrote the destination"
    );
    // And it left no staging file behind next to it.
    let strays: Vec<_> = std::fs::read_dir(&tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("roadmark-tmp"))
        .collect();
    assert!(strays.is_empty(), "left staging files: {strays:?}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn determinism_round_trip() {
    let root = fixture("minimal");
    let config = roadmark::load_config(&root).unwrap();
    let mut a = roadmark::load_features(&root).unwrap();
    let mut b = roadmark::load_features(&root).unwrap();
    roadmark::sort_features(&mut a, &config);
    roadmark::sort_features(&mut b, &config);
    assert_eq!(roadmark::render(&a, &config), roadmark::render(&b, &config));
}
