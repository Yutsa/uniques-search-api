use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;
use uniques_http_api::{
    app, load_app_state, load_app_state_from_http, load_app_state_from_object_store, load_env,
    load_settings, spawn_formats_hot_reload, spawn_hot_reload, AnyIndexSource, DiskIndexSource,
    HttpIndexClient, IndexSourceKind, ObjectStoreIndexClient, RemoteIndexSource, ServerState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env();
    let settings = Arc::new(load_settings()?);

    let (app_state, reload_source) = load_app_state_and_reload_source(&settings)?;
    let server = ServerState {
        app: Arc::new(app_state),
        settings: Arc::clone(&settings),
    };

    if settings.index.reload.enabled {
        if let Some(source) = reload_source {
            spawn_hot_reload(
                Arc::clone(&server.app),
                Arc::clone(&settings),
                source,
                settings.index.reload.interval_secs()?,
            );
        }
    }

    if let Some(interval_secs) = settings
        .formats
        .as_ref()
        .and_then(|f| f.reload_interval_secs())
    {
        spawn_formats_hot_reload(
            Arc::clone(&server.app),
            Arc::clone(&settings),
            interval_secs,
        );
    }

    let router = app(server);
    let addr = format!("0.0.0.0:{}", settings.server.port);
    let listener = TcpListener::bind(&addr).await?;
    println!("Server started successfully at {addr}");
    axum::serve(listener, router).await?;
    Ok(())
}

fn load_app_state_and_reload_source(
    settings: &uniques_http_api::Settings,
) -> anyhow::Result<(uniques_http_api::AppState, Option<AnyIndexSource>)> {
    match settings.index.source {
        IndexSourceKind::Disk | IndexSourceKind::Archive => {
            let path = settings.index_path()?;
            let reload_source = settings
                .index
                .reload
                .enabled
                .then(|| AnyIndexSource::Disk(DiskIndexSource::new(path.clone())));
            Ok((load_app_state(settings)?, reload_source))
        }
        IndexSourceKind::ObjectStore => {
            let url = settings.object_store_url()?.to_string();
            let client = ObjectStoreIndexClient::new(&url)
                .with_context(|| format!("connect to object store at {url}"))?;
            let reload_source = settings
                .index
                .reload
                .enabled
                .then(|| AnyIndexSource::Remote(RemoteIndexSource::new(client.clone())));
            Ok((
                load_app_state_from_object_store(&client, settings)?,
                reload_source,
            ))
        }
        IndexSourceKind::Http => {
            // No hot-reload yet (rejected at config validation time), so no reload source.
            let url = settings.http_index_url()?.to_string();
            let client = HttpIndexClient::new(url);
            Ok((load_app_state_from_http(&client, settings)?, None))
        }
    }
}
