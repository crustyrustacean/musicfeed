// src/app.rs

use crate::configuration::{BasicAuthSettings, CorsSettings};
use crate::database::DatabaseBackend;
use crate::metadata::MetadataClient;
use crate::middleware::basic_auth;
use crate::routes::{
    get_artist_stats, get_index_page, get_rotation_entry, health_check, post_rotation_entry_ds,
};
use crate::shutdown_signal;
use crate::telemetry::request_id;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::dev::Server;
use actix_web::http::header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue};
use actix_web::{App, HttpServer, middleware, web};
use std::net::TcpListener;
use std::sync::Arc;
use tera::Tera;
use tracing::info;
use tracing_actix_web::TracingLogger;

const DATASTAR_REQUEST: HeaderName = HeaderName::from_static("datastar-request");

pub struct Application {
    port: u16,
    server: Server,
}

impl Application {
    pub async fn build(
        addr: &str,
        database: Arc<dyn DatabaseBackend>,
        metadata: MetadataClient,
        basicauth: BasicAuthSettings,
        cors_settings: CorsSettings,
    ) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        info!(address = %listener.local_addr()?, "listening");

        let mut templates = Tera::default();
        templates
            .load_from_glob("templates/**/*.html")
            .expect("Unable to load the Tera templates.");

        let templates = web::Data::new(templates);
        let database = web::Data::new(database);
        let metadata = web::Data::new(metadata);
        let basicauth = web::Data::new(basicauth);
        let cors_origins = cors_settings
            .allowed_origins
            .iter()
            .map(|origin| {
                HeaderValue::from_str(origin).map_err(|_| {
                    anyhow::anyhow!("invalid CORS origin in configuration: {origin:?}")
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let server = HttpServer::new(move || {
            let templates = templates.clone();
            let database = database.clone();
            let metadata = metadata.clone();
            let basicauth = basicauth.clone();
            let cors_origins = cors_origins.clone();
            let cors = Cors::default()
                .allowed_methods(["GET", "HEAD"])
                .allowed_headers([CONTENT_TYPE, ACCEPT, DATASTAR_REQUEST])
                .allowed_origin_fn(move |origin, _request_head| {
                    cors_origins.iter().any(|allowed| allowed == origin)
                });

            App::new()
                .app_data(templates)
                .app_data(database)
                .app_data(metadata)
                .app_data(basicauth)
                .wrap(middleware::from_fn(request_id))
                .wrap(TracingLogger::default())
                // CORS must wrap the entire application so preflight requests receive
                // headers before route or authentication middleware can reject them.
                .wrap(cors)
                .service(web::resource("/").route(web::get().to(get_index_page)))
                // `/health_check` is the original route and is what railway.toml points
                // at; `/health` is the conventional spelling, kept as an alias.
                .service(web::resource("/health_check").route(web::get().to(health_check)))
                .service(web::resource("/health").route(web::get().to(health_check)))
                .service(
                    web::resource("/rotation")
                        .route(web::get().to(get_rotation_entry))
                        .route(
                            web::post()
                                .to(post_rotation_entry_ds)
                                .wrap(middleware::from_fn(basic_auth)),
                        ),
                )
                .service(web::resource("/stats").route(web::get().to(get_artist_stats)))
                .service(Files::new("/static", "static"))
        })
        .listen(listener)?
        .run();

        Ok(Self { port, server })
    }

    pub fn port(&self) -> std::io::Result<u16> {
        Ok(self.port)
    }

    pub async fn run_until_stopped(self) -> std::io::Result<()> {
        let server = self.server;
        let handle = server.handle();

        tokio::select! {
            result = server => result?,
            _ = shutdown_signal() => {
                handle.stop(true).await;
            }
        }

        tracing::info!("shutdown complete");
        Ok(())
    }
}
