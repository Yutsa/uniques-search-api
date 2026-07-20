use index_core::add_extra_filter::{add_extra_filter, AddExtraFilterOptions};
use index_core::build;
use index_core::extra_catalog::{ExtraCatalog, ExtraFilterType, EXTRA_CATALOG_FILE, EXTRA_DIR};
use roaring::RoaringBitmap;
use std::fs;
use std::path::PathBuf;

fn setup_fixture_dataset() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../index-core/tests/card-json");

    let cards = [
        ("AX", "06", "ALT_COREKS_B_AX_06_U_5.json"),
        ("MU", "22", "ALT_COREKS_B_MU_22_U_3140.json"),
        ("OR", "16", "ALT_COREKS_B_OR_16_U_6.json"),
    ];

    for (faction, family, file) in cards {
        let dest_dir = root
            .join("json")
            .join("COREKS")
            .join(faction)
            .join(family);
        fs::create_dir_all(&dest_dir).expect("mkdir");
        fs::copy(fixtures.join(file), dest_dir.join(file)).expect("copy");
    }

    (dir, root)
}

fn build_fixture_index() -> (tempfile::TempDir, PathBuf) {
    let (_guard, root) = setup_fixture_dataset();
    let out = tempfile::tempdir().expect("out tempdir");
    build::build(
        &root,
        "COREKS",
        out.path(),
        build::BuildOptions {
            file_limit: None,
            profile: false,
            ..Default::default()
        },
    )
    .expect("build");
    let path = out.path().join("COREKS");
    (out, path)
}

#[test]
fn add_extra_filter_writes_bitmap_and_catalog() {
    let (_index_guard, index_dir) = build_fixture_index();

    let refs_file = tempfile::NamedTempFile::new().expect("refs temp");
    fs::write(
        refs_file.path(),
        "# include list\nALT_COREKS_B_AX_06_U_5\nALT_COREKS_B_OR_16_U_6\n",
    )
    .expect("write refs");

    let summary = add_extra_filter(&AddExtraFilterOptions {
        index_dir: index_dir.clone(),
        filter_id: "test-include".to_string(),
        refs_file: refs_file.path().to_path_buf(),
        filter_type: Some(ExtraFilterType::Format),
        negated: false,
        replace: false,
    })
    .expect("add extra filter");

    assert_eq!(summary.filter_id, "test-include");
    assert_eq!(summary.refs_read, 2);
    assert_eq!(summary.card_count, 2);
    assert!(!summary.negated);
    assert_eq!(summary.filter_type, Some(ExtraFilterType::Format));

    let roar_path = index_dir.join(EXTRA_DIR).join("test-include.roar");
    assert!(roar_path.is_file());
    let bytes = fs::read(&roar_path).expect("read roar");
    let bitmap = RoaringBitmap::deserialize_from(&bytes[..]).expect("deserialize");
    assert_eq!(bitmap.len(), 2);

    let catalog = ExtraCatalog::load(&index_dir.join(EXTRA_CATALOG_FILE)).expect("extra catalog");
    assert_eq!(catalog.set, "COREKS");
    assert_eq!(catalog.entries.len(), 1);
    let entry = &catalog.entries[0];
    assert_eq!(entry.id, "test-include");
    assert_eq!(entry.r#type, Some(ExtraFilterType::Format));
    assert!(!entry.negated);
    assert_eq!(entry.card_count, 2);
    assert_eq!(entry.bitmap_bytes, summary.bitmap_bytes);
    assert_eq!(entry.bitmap_file, "extra/test-include.roar");
}

#[test]
fn add_extra_filter_negated_and_duplicate_id_errors() {
    let (_index_guard, index_dir) = build_fixture_index();

    let refs_file = tempfile::NamedTempFile::new().expect("refs temp");
    fs::write(refs_file.path(), "ALT_COREKS_B_MU_22_U_3140\n").expect("write refs");

    add_extra_filter(&AddExtraFilterOptions {
        index_dir: index_dir.clone(),
        filter_id: "exclude-one".to_string(),
        refs_file: refs_file.path().to_path_buf(),
        filter_type: Some(ExtraFilterType::Property),
        negated: true,
        replace: false,
    })
    .expect("first add");

    let catalog = ExtraCatalog::load(&index_dir.join(EXTRA_CATALOG_FILE)).expect("extra catalog");
    assert!(catalog.entries[0].negated);
    assert_eq!(catalog.entries[0].r#type, Some(ExtraFilterType::Property));

    let err = add_extra_filter(&AddExtraFilterOptions {
        index_dir,
        filter_id: "exclude-one".to_string(),
        refs_file: refs_file.path().to_path_buf(),
        filter_type: None,
        negated: false,
        replace: false,
    })
    .expect_err("duplicate filter id");
    assert!(
        err.to_string().contains("already exists"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn add_extra_filter_replace_overwrites_bitmap_and_catalog() {
    let (_index_guard, index_dir) = build_fixture_index();

    let refs_one = tempfile::NamedTempFile::new().expect("refs temp");
    fs::write(refs_one.path(), "ALT_COREKS_B_AX_06_U_5\n").expect("write refs");

    add_extra_filter(&AddExtraFilterOptions {
        index_dir: index_dir.clone(),
        filter_id: "my-filter".to_string(),
        refs_file: refs_one.path().to_path_buf(),
        filter_type: Some(ExtraFilterType::Property),
        negated: false,
        replace: false,
    })
    .expect("initial add");

    let refs_two = tempfile::NamedTempFile::new().expect("refs temp");
    fs::write(
        refs_two.path(),
        "ALT_COREKS_B_AX_06_U_5\nALT_COREKS_B_OR_16_U_6\n",
    )
    .expect("write refs");

    let summary = add_extra_filter(&AddExtraFilterOptions {
        index_dir: index_dir.clone(),
        filter_id: "my-filter".to_string(),
        refs_file: refs_two.path().to_path_buf(),
        filter_type: Some(ExtraFilterType::Format),
        negated: true,
        replace: true,
    })
    .expect("replace");

    assert!(summary.replaced);
    assert_eq!(summary.card_count, 2);
    assert!(summary.negated);

    let catalog = ExtraCatalog::load(&index_dir.join(EXTRA_CATALOG_FILE)).expect("extra catalog");
    assert_eq!(catalog.entries.len(), 1);
    let entry = &catalog.entries[0];
    assert_eq!(entry.r#type, Some(ExtraFilterType::Format));
    assert!(entry.negated);
    assert_eq!(entry.card_count, 2);
}
