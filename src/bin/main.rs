// src/main.rs

use musicfeed::Application;
use musicfeed::database::{DatabaseBackend, SqliteRepository};
use musicfeed::get_configuration;
use musicfeed::metadata::MetadataClient;
use musicfeed::telemetry::{get_subscriber, init_subscriber};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // initialize tracing
    let subscriber = get_subscriber("musicfeed".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);

    // read the application settings
    let settings = get_configuration()?;

    let database: Arc<dyn DatabaseBackend> =
        Arc::new(SqliteRepository::new(&settings.database).await?);
    let metadata = MetadataClient::with_base_urls(
        settings.metadata.musicbrainz_base_url.clone(),
        settings.metadata.cover_art_base_url.clone(),
    );

    // The healer is the process's second employee: it starts when the process
    // starts (pass one on wake), then keeps its own timer.
    musicfeed::healer::spawn_healer(database.clone(), metadata.clone(), settings.healing);

    let app_address = format!(
        "{}:{}",
        settings.application.host, settings.application.port
    );

    // Build the application. Actix registers each shared component as its own
    // `web::Data` value, so handlers only receive the dependencies they use.
    let app = Application::build(
        &app_address,
        database,
        metadata,
        settings.basicauth,
        settings.cors,
    )
    .await?;

    // run the application
    app.run_until_stopped().await?;

    Ok(())
}
