// tests/api/helpers.rs

use musicfeed::Application;
use musicfeed::configuration::{DatabaseSettings, get_configuration};
use musicfeed::database::{DatabaseBackend, SqliteRepository};
use musicfeed::metadata::MetadataClient;
use musicfeed::telemetry::{get_subscriber, init_subscriber};
use std::sync::{Arc, LazyLock};

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: LazyLock<()> = LazyLock::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    };
});

#[allow(dead_code)]
pub struct TestApp {
    pub address: String,
    pub port: u16,
    /// Sends Basic credentials, for the routes that require them.
    pub api_client: reqwest::Client,
    /// Sends no credentials. GET routes are deliberately ungated — the blog
    /// island fetches them from a static site that has nowhere to keep a secret —
    /// so tests asserting on those routes must use this client. If it were to
    /// send credentials anyway, a future regression that gated the GETs would
    /// pass unnoticed.
    pub unauthenticated_client: reqwest::Client,
    /// The metadata stubs, kept alive for as long as the app under test.
    /// Dropping a `MockServer` shuts it down, so these must outlive every request.
    pub metadata_stub: crate::metadata_stub::MetadataStubs,
    /// The store's scratch directory. Held (not merely remembered) so the
    /// SQLite file exists for as long as the app under test does.
    pub database_dir: tempfile::TempDir,
}

/// Spin up the app with the metadata services stubbed.
///
/// **Default for every test.** Pointing at a local stub keeps the suite fast and
/// hermetic: no network, no rate limits, and no failing tests because
/// MusicBrainz is briefly unhappy. The stub defaults to a plain miss, so a
/// lookup yields no cover or year — which most tests do not care about.
///
/// To register a specific response, start a `MockServer`, mount your mocks on it,
/// and call [`spawn_app_against`] with it.
pub async fn spawn_app() -> TestApp {
    let stub = crate::metadata_stub::MetadataStubs::miss().await;
    spawn_app_against(stub).await
}

/// Spin up the app pointed at an already-configured metadata stub.
///
/// Takes the stub by value so it can be stored on the returned [`TestApp`],
/// which keeps it alive for the duration of the test.
pub async fn spawn_app_against(metadata_stub: crate::metadata_stub::MetadataStubs) -> TestApp {
    LazyLock::force(&TRACING);

    let configuration = get_configuration().expect("Failed to read configuration");
    let app_address = format!("{}:{}", configuration.application.host, 0);

    // One throwaway store per app. A fresh file per test keeps the suite
    // parallel-safe: no test can see another's rotation.
    let database_dir = tempfile::tempdir().expect("Failed to create a temp directory");
    let database_settings = DatabaseSettings {
        path: database_dir.path().join("test.db").display().to_string(),
        max_connections: Some(1),
    };

    let database: Arc<dyn DatabaseBackend> =
        Arc::new(SqliteRepository::new(&database_settings).await.unwrap());
    let metadata = MetadataClient::with_base_urls(
        metadata_stub.musicbrainz.uri(),
        metadata_stub.cover_art.uri(),
    );

    build_test_app(
        app_address,
        database,
        metadata,
        metadata_stub,
        &configuration.basicauth,
        database_dir,
    )
    .await
}

/// Spin up the app over a *specific* store file.
///
/// The persistence test's needs: spawn an application, post an entry, drop it,
/// spawn a second application over the same file, and assert the entry
/// survived. Nothing else in the harness cares about the path.
pub async fn spawn_app_at(db_path: impl AsRef<std::path::Path>) -> TestApp {
    LazyLock::force(&TRACING);

    let configuration = get_configuration().expect("Failed to read configuration");
    let app_address = format!("{}:{}", configuration.application.host, 0);

    let database_settings = DatabaseSettings {
        path: db_path.as_ref().display().to_string(),
        max_connections: Some(1),
    };

    let stub = crate::metadata_stub::MetadataStubs::miss().await;
    let database: Arc<dyn DatabaseBackend> =
        Arc::new(SqliteRepository::new(&database_settings).await.unwrap());
    let metadata = MetadataClient::with_base_urls(stub.musicbrainz.uri(), stub.cover_art.uri());

    build_test_app(
        app_address,
        database,
        metadata,
        stub,
        &configuration.basicauth,
        tempfile::tempdir().expect("temp dir"),
    )
    .await
}

// --- Server-sent events ---
//
// The rotation and stats routes answer with SSE streams of Datastar patches.
// These helpers let tests assert on the wire contract — transport headers,
// blank-line framing, event order and dataline payloads — without reaching
// into the app's internal representation of a patch.

/// One parsed SSE frame: the `event:` name (if any) and every `data:` line's
/// value, in wire order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: Vec<String>,
}

impl SseEvent {
    /// The value of the first dataline named `key`, e.g. `selector` or `mode`.
    ///
    /// Datastar datalines look like `data: selector #rotation-list`, so the
    /// value is everything after `key `.
    pub fn dataline(&self, key: &str) -> Option<&str> {
        self.data.iter().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(' '))
        })
    }
}

/// Parse an SSE wire body into its frames.
///
/// Per the SSE spec a frame is a block of `field: value` lines terminated by
/// one blank line. Anything after the last blank line is an unterminated frame
/// and is dropped — callers that care should also assert the body ends with
/// `\n\n`, which [`assert_sse_framing`] does.
pub fn parse_sse(body: &str) -> Vec<SseEvent> {
    let fresh = || SseEvent {
        event: None,
        data: Vec::new(),
    };
    let mut events = Vec::new();
    let mut event = fresh();
    for line in body.lines() {
        if line.is_empty() {
            if event.event.is_some() || !event.data.is_empty() {
                events.push(std::mem::replace(&mut event, fresh()));
            }
            continue;
        }
        if let Some(name) = line.strip_prefix("event: ") {
            assert!(
                event.event.replace(name.to_owned()).is_none(),
                "multiple event names without a blank-line separator:\n{body}"
            );
        } else if let Some(data) = line.strip_prefix("data: ") {
            event.data.push(data.to_owned());
        }
        // Other fields (`id:`, `retry:`, comments) are not part of this contract.
    }
    events
}

/// Assert the response carries the transport headers every SSE consumer
/// requires: without `text/event-stream` the browser will not treat the body as
/// an event stream at all, and `no-cache` keeps intermediaries from swallowing
/// events on a replay.
pub fn assert_sse_headers(response: &reqwest::Response) {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .expect("SSE response must carry a content-type")
        .to_str()
        .expect("content-type must be ASCII");
    assert!(
        content_type.split(';').next().unwrap().trim() == "text/event-stream",
        "expected text/event-stream, got: {content_type}"
    );

    let cache_control = response
        .headers()
        .get(reqwest::header::CACHE_CONTROL)
        .expect("SSE response must carry cache-control")
        .to_str()
        .expect("cache-control must be ASCII");
    assert!(
        cache_control
            .split(',')
            .any(|directive| directive.trim().eq_ignore_ascii_case("no-cache")),
        "expected no-cache, got: {cache_control}"
    );
}

/// Read an SSE response body to completion under a hard bound.
///
/// The routes under test send finite streams, so the response body must finish
/// on its own; a regression to an open-ended stream fails here instead of hanging
/// the suite forever.
pub async fn sse_body(response: reqwest::Response) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(10), response.text())
        .await
        .expect("the stream should end on its own within 10 seconds")
        .expect("Failed to read the stream")
}

/// Assert the body is a stream of exactly `expected` well-framed SSE events and
/// return them in wire order.
///
/// Every frame must be closed by a blank line — including the last, so the
/// finite stream ends cleanly rather than leaving a dangling frame — and every
/// frame must name its event type.
pub fn assert_sse_framing(body: &str, expected: usize) -> Vec<SseEvent> {
    // SSE allows LF, CRLF and CR line endings; test event boundaries, not the
    // encoder's choice of newline.
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    assert!(
        normalized.ends_with("\n\n"),
        "the final event must be terminated by a blank line:\n{body}"
    );
    let events = parse_sse(&normalized);
    assert_eq!(
        events.len(),
        expected,
        "expected {expected} blank-line-delimited events:\n{body}"
    );
    assert!(
        events.iter().all(|e| e.event.is_some()),
        "every event must name its type:\n{body}"
    );
    events
}

/// Shared construction: bind port 0, spawn the server, build the clients.
async fn build_test_app(
    app_address: String,
    database: Arc<dyn DatabaseBackend>,
    metadata: MetadataClient,
    metadata_stub: crate::metadata_stub::MetadataStubs,
    basicauth: &musicfeed::configuration::BasicAuthSettings,
    database_dir: tempfile::TempDir,
) -> TestApp {
    let configuration = get_configuration().expect("Failed to read configuration");
    let application = Application::build(
        &app_address,
        database,
        metadata,
        configuration.basicauth.clone(),
        configuration.cors,
    )
    .await
    .expect("Unable to build the application");

    let application_port = application
        .port()
        .expect("Unable to obtain the application port");
    // Spawn-and-forget: the server runs until the test process ends. The
    // JoinHandle is dropped on purpose — nothing joins a test server.
    tokio::spawn(application.run_until_stopped());

    // Two clients, two purposes. The authenticated one sends the Basic
    // credentials on every request; the bare one documents that the GETs are open.
    // reqwest has no per-client credential setting, so the header is pre-computed
    // once and attached as a default — the browser equivalent of a cached login.
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", basicauth.username, basicauth.password));
    let auth_header = format!("Basic {encoded}");

    let api_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(
            std::iter::once((
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&auth_header).unwrap(),
            ))
            .collect(),
        )
        .build()
        .unwrap();
    let unauthenticated_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    TestApp {
        address: format!("http://localhost:{}", application_port),
        port: application_port,
        api_client,
        unauthenticated_client,
        metadata_stub,
        database_dir,
    }
}
