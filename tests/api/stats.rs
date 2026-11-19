// tests/api/stats.rs

use crate::helpers::{assert_sse_framing, assert_sse_headers, spawn_app, sse_body};
use musicfeed::database::{DatabaseBackend, SqliteRepository};
use musicfeed::domain::NewRotationEntry;

async fn store() -> (tempfile::TempDir, SqliteRepository) {
    let dir = tempfile::tempdir().expect("temp dir");
    let settings = musicfeed::configuration::DatabaseSettings {
        path: dir.path().join("test.db").display().to_string(),
        max_connections: Some(1),
    };
    let repo = SqliteRepository::new(&settings)
        .await
        .expect("store connects and migrates");
    (dir, repo)
}

fn entry(artist: &str, album: &str) -> NewRotationEntry {
    NewRotationEntry {
        listened_date: jiff::civil::date(2026, 11, 1),
        artist: artist.to_string(),
        album: album.to_string(),
        cover: None,
        year: None,
        note: None,
    }
}

#[tokio::test]
async fn artist_stats_counts_spins_per_artist() {
    // Arrange
    let (_dir, repo) = store().await;
    for (artist, album) in [
        ("Saxon", "Wheels of Steel"),
        ("Saxon", "Denim and Leather"),
        ("Judas Priest", "Painkiller"),
    ] {
        repo.insert(entry(artist, album)).await.expect("insert");
    }

    // Act
    let stats = repo.artist_stats(20).await.expect("stats");

    // Assert
    assert_eq!(stats.len(), 2);
    assert_eq!(stats[0].artist, "Saxon");
    assert_eq!(stats[0].spins, 2);
    assert_eq!(stats[1].artist, "Judas Priest");
    assert_eq!(stats[1].spins, 1);
}

#[tokio::test]
async fn artist_stats_on_empty_store_is_empty_not_an_error() {
    // Arrange
    let (_dir, repo) = store().await;

    // Act
    let stats = repo.artist_stats(20).await.expect("stats");

    // Assert
    assert!(stats.is_empty());
}

#[tokio::test]
async fn artist_stats_respects_the_limit() {
    // Arrange
    let (_dir, repo) = store().await;
    for artist in ["Metallica", "Saxon", "Slayer"] {
        repo.insert(entry(artist, "An Album"))
            .await
            .expect("insert");
    }

    // Act
    let stats = repo.artist_stats(2).await.expect("stats");

    // Assert
    assert_eq!(stats.len(), 2);
}

#[tokio::test]
async fn artist_stats_breaks_ties_by_artist_name() {
    // Arrange: every artist has one spin, so only the tiebreak decides order.
    let (_dir, repo) = store().await;
    for artist in ["Saxon", "Accept", "Metallica"] {
        repo.insert(entry(artist, "An Album"))
            .await
            .expect("insert");
    }

    // Act
    let stats = repo.artist_stats(20).await.expect("stats");

    // Assert
    let artists: Vec<&str> = stats.iter().map(|s| s.artist.as_str()).collect();
    assert_eq!(artists, ["Accept", "Metallica", "Saxon"]);
}

#[tokio::test]
async fn get_artist_stats_appends_widget_and_clears_placeholder() {
    // Arrange
    let app = spawn_app().await;
    app.api_client
        .post(format!("{}/rotation", app.address))
        .json(&serde_json::json!({
            "artist": "Sabaton",
            "album": "Attero Dominatus",
            "note": ""
        }))
        .send()
        .await
        .expect("Failed to execute request.");

    // Act
    let response = app
        .unauthenticated_client
        .get(format!("{}/stats", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    assert!(body.contains("event: datastar-patch-elements"));
    assert!(body.contains("data: selector #artist-stats-list"));
    assert!(body.contains("data: mode append"));
    assert!(body.contains("artist-stats-widget"));
    assert!(body.contains("<ol class=\"artist-stats-list\">"));
    assert!(body.contains("Sabaton"));
    assert!(body.contains(r#"class="artist-stats-spins">1<"#));
    assert!(body.contains("data: selector #artist-stats-loading"));
    assert!(body.contains("data: mode remove"));
}

#[tokio::test]
async fn get_artist_stats_renders_empty_state_without_entries() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app
        .unauthenticated_client
        .get(format!("{}/stats", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    assert!(body.contains("No spins recorded yet."));
}

#[tokio::test]
async fn get_artist_stats_streams_a_well_framed_finite_sse_response() {
    // Arrange
    let app = spawn_app().await;
    let seed = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&serde_json::json!({
            "artist": "Sabaton",
            "album": "Attero Dominatus",
            "note": ""
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .expect("Failed to execute request.");
    assert!(seed.status().is_success());
    sse_body(seed).await;

    // Act — the blog island fetches this unauthenticated, on purpose
    let response = app
        .unauthenticated_client
        .get(format!("{}/stats", app.address))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — transport headers, framing, and a stream that ends on its own
    assert!(response.status().is_success());
    assert_sse_headers(&response);
    let body = sse_body(response).await;

    // Two events: the append, then the placeholder removal.
    let events = assert_sse_framing(&body, 2);
    let html = events[0].dataline("elements").expect("stats HTML payload");
    assert!(html.contains("artist-stats-widget"));
    assert!(html.contains("Sabaton"));
    assert!(events[1].dataline("elements").is_none());
    assert_eq!(
        events[0].event.as_deref(),
        Some("datastar-patch-elements"),
        "the first event must patch elements:\n{body}"
    );
    assert_eq!(
        events[1].event.as_deref(),
        Some("datastar-patch-elements"),
        "the second event must patch elements:\n{body}"
    );

    // ...in that order: append the leaderboard, then clear its placeholder.
    assert_eq!(
        events[0].dataline("selector"),
        Some("#artist-stats-list"),
        "the append must target the stats list:\n{body}"
    );
    assert_eq!(
        events[0].dataline("mode"),
        Some("append"),
        "the append must be an append:\n{body}"
    );
    assert_eq!(
        events[1].dataline("selector"),
        Some("#artist-stats-loading"),
        "the removal must target the stats placeholder:\n{body}"
    );
    assert_eq!(
        events[1].dataline("mode"),
        Some("remove"),
        "the placeholder event must be a removal:\n{body}"
    );

    // The rendered leaderboard must arrive as ONE dataline; multi-line splits
    // would make Datastar patch fragments instead of the whole widget.
    assert_eq!(
        events[0]
            .data
            .iter()
            .filter(|line| line.starts_with("elements "))
            .count(),
        1,
        "the stats widget must arrive as a single `elements` dataline:\n{body}"
    );
}
