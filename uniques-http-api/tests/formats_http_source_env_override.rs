use uniques_http_api::{FormatsSourceConfig, Settings};

#[test]
fn formats_http_source_settable_purely_via_env_vars() {
    unsafe {
        std::env::set_var("FORMATS__SOURCE__TYPE", "http");
        std::env::set_var(
            "FORMATS__SOURCE__MANIFEST_URL",
            "https://example.com/manifest.json",
        );
        std::env::set_var("FORMATS__RELOAD_INTERVAL_SECS", "300");
    }

    let toml = r#"
        [server]
        port = 3000
        [index]
        source = "disk"
        path = "./index"
        [index.reload]
        enabled = false
    "#;

    let settings: Settings = config::Config::builder()
        .add_source(config::File::from_str(toml, config::FileFormat::Toml))
        .add_source(config::Environment::default().separator("__"))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap();

    unsafe {
        std::env::remove_var("FORMATS__SOURCE__TYPE");
        std::env::remove_var("FORMATS__SOURCE__MANIFEST_URL");
        std::env::remove_var("FORMATS__RELOAD_INTERVAL_SECS");
    }

    let formats = settings.formats.expect("formats should populate from env alone");
    match formats.source {
        FormatsSourceConfig::Http { manifest_url } => {
            assert_eq!(manifest_url, "https://example.com/manifest.json");
        }
        other => panic!("expected Http source, got {other:?}"),
    }
    assert_eq!(formats.reload_interval_secs, 300);
}
