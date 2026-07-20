use index_core::build::{self, BuildOptions};
use std::fs;
use std::path::PathBuf;

fn setup_many_files(count: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../index-core/tests/card-json");
    let source = fixtures.join("ALT_COREKS_B_AX_06_U_5.json");

    for i in 1..=count {
        let dest_dir = root.join("json").join("COREKS").join("AX").join("06");
        fs::create_dir_all(&dest_dir).expect("mkdir");
        let dest = dest_dir.join(format!("ALT_COREKS_B_AX_06_U_{i}.json"));
        fs::copy(&source, &dest).expect("copy");
    }

    (dir, root)
}

#[test]
fn discovery_stops_at_limit() {
    let (_guard, root) = setup_many_files(15);
    let out = tempfile::tempdir().expect("out");
    let summary = build::build(
        &root,
        "COREKS",
        out.path(),
        BuildOptions {
            file_limit: Some(10),
            profile: false,
            ..Default::default()
        },
    )
    .expect("build");

    assert_eq!(summary.files_processed, 10);
    assert!(summary.stopped_early);
}
