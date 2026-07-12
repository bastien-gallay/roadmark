//! Integration test: load fixture .roadmap/, generate, snapshot.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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
