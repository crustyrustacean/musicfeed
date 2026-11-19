// tests/api/rotation.rs

use crate::helpers::{
    assert_sse_framing, assert_sse_headers, spawn_app, spawn_app_against, sse_body,
};
use crate::index::{
    attribute, elements, has_class, has_zero_year, is_hidden, row_with_id, table_body,
};

use base64::Engine;
use reqwest::StatusCode;

/// Only what the form actually sends. `cover` and `year` are looked up, not typed.
fn payload() -> serde_json::Value {
    serde_json::json!({
        "artist": "Sabaton",
        "album": "Attero Dominatus",
        "note": "A co-worker turned me on to Sabaton in early 2022."
    })
}

#[tokio::test]
async fn post_rotation_entry_returns_append_patch_and_clears_signals() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    assert!(body.contains("event: datastar-patch-elements"));
    assert!(body.contains("data: selector #rotation-list"));
    assert!(body.contains("data: mode append"));
    // the app stamps id 0 for the first entry and the date itself
    assert!(
        body.contains(r#"class="rotation-artist""#),
        "body was: {body}"
    );
    assert!(body.contains("Sabaton"));
    assert!(body.contains("Attero Dominatus"));
    // and every bound signal is reset so the form is ready for the next entry
    assert!(body.contains("event: datastar-patch-signals"));
    assert!(body.contains(r#""artist":"""#));
    assert!(body.contains(r#""album":"""#));
}

#[tokio::test]
async fn cleared_signals_must_be_deserializable() {
    // Arrange
    let app = spawn_app().await;

    // Act — read the clear patch the server actually sends, then push its values
    // straight back into the next submission, exactly as the browser would.
    let first = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.")
        .text()
        .await
        .unwrap();

    let signals: serde_json::Value = first
        .lines()
        .find_map(|line| line.strip_prefix("data: signals "))
        .expect("stream carried no signals patch")
        .parse()
        .expect("signals patch was not valid JSON");

    // Merge the cleared values over a complete payload, as the form's bound
    // signals would be when the user submits again.
    let mut second = payload();
    for (key, value) in signals.as_object().unwrap() {
        second[key.as_str()] = value.clone();
    }

    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&second)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — the previous submission's own reset must not poison the next one.
    // Clearing `year` to `""` made this a 400 and silently added nothing.
    assert!(
        response.status().is_success(),
        "cleared signals were rejected on resubmit ({}): {second}",
        response.status()
    );

    // And it must be the *second* entry, proving the first was not re-sent.
    let body = response.text().await.unwrap();
    assert!(
        body.contains(r#"class="rotation-artist""#),
        "expected the second entry, got: {body}"
    );
}
#[tokio::test]
async fn posted_entry_appears_in_index_list() {
    // Arrange — a stubbed hit, so the entry has a year to assert on
    let mb_id = "b4974b0b-305e-4f98-b16d-04112f3cced2";
    let stubs = crate::metadata_stub::MetadataStubs::full_hit(mb_id).await;
    let app = spawn_app_against(stubs).await;

    // Act
    let before = jiff::ZonedDateTime::now().date().to_string();
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");
    assert!(response.status().is_success());
    let after = jiff::ZonedDateTime::now().date().to_string();

    let body = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();

    // Assert — the initial page renders the shared table row with its metadata.
    let body = table_body(&body);
    let (tag, row) = row_with_id(body, "rotation-entry-1");
    assert!(has_class(tag, "rotation-row"));
    let cells = elements(row, "td");
    assert_eq!(cells.len(), 6, "expected six cells: {row}");
    assert!(cells[1].1.contains("Sabaton"));
    assert!(cells[2].1.contains("Attero Dominatus"));
    assert!(cells[3].1.contains("2006"));
    assert!(
        cells[4]
            .1
            .contains("A co-worker turned me on to Sabaton in early 2022.")
    );
    assert!(
        cells[5].1.contains(&before) || cells[5].1.contains(&after),
        "listened date is missing: {row}"
    );
}

#[tokio::test]
async fn an_empty_note_is_omitted_rather_than_rendered_empty() {
    // Arrange — a blank note is legitimate, so it must not become an empty
    // paragraph on the page. `cover` and `year` are looked up, not sent, so
    // there is nothing for them here.
    let app = spawn_app().await;
    let mut body = payload();
    body["note"] = serde_json::json!("");

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — the entry still saves, it just carries no note markup
    assert!(response.status().is_success());
    let sse_body = response.text().await.unwrap();
    assert!(sse_body.contains("Sabaton"));
    assert!(!sse_body.contains("rotation-note"), "sse was: {sse_body}");
}

#[tokio::test]
async fn cover_and_year_cannot_be_supplied_by_the_client() {
    // Arrange
    let app = spawn_app().await;
    let mut body = payload();
    // Both are app-controlled now. A client that sends them must not win.
    body["cover"] = serde_json::json!("/static/images/covers/someone-elses-choice.jpg");
    body["year"] = serde_json::json!(1066);

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — the entry is saved, but the forged values are ignored entirely.
    assert!(response.status().is_success());
    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();
    assert!(
        !page.contains("someone-elses-choice"),
        "a client-supplied cover was accepted:\n{page}"
    );
    assert!(
        !page.contains("1066"),
        "a client-supplied year was accepted:\n{page}"
    );
}

#[tokio::test]
async fn a_successful_lookup_fills_in_the_cover_and_year() {
    // Arrange — a stubbed MusicBrainz hit, including a back cover so the
    // front-cover filter is genuinely exercised.
    let mb_id = "b4974b0b-305e-4f98-b16d-04112f3cced2";
    let stubs = crate::metadata_stub::MetadataStubs::full_hit(mb_id).await;
    let app = spawn_app_against(stubs).await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — the entry carries the looked-up year and cover
    assert!(response.status().is_success());
    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();
    let (tag, row) = row_with_id(table_body(&page), "rotation-entry-1");
    assert!(has_class(tag, "rotation-row"));
    let cells = elements(row, "td");
    assert_eq!(cells.len(), 6, "expected six cells: {row}");
    assert!(
        cells[0].1.contains("front-250.jpg"),
        "the chosen cover is missing: {row}"
    );
    assert!(
        cells[3].1.contains("2006"),
        "the looked-up year is missing: {row}"
    );
}

#[tokio::test]
async fn a_cover_is_absent_when_the_release_has_no_art() {
    // Arrange — the release exists but the archive has nothing for it.
    let mb_id = "bootleg-0001";
    let stubs = crate::metadata_stub::MetadataStubs::start().await;
    stubs.expect_no_art(mb_id).await;
    let app = spawn_app_against(stubs).await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — still saved, with the year but no image
    assert!(response.status().is_success());
    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();
    let (_, row) = row_with_id(table_body(&page), "rotation-entry-1");
    let cells = elements(row, "td");
    assert_eq!(
        cells.len(),
        6,
        "missing artwork must not remove a column: {row}"
    );
    assert!(
        cells[3].1.contains("2006"),
        "the year should still land: {row}"
    );
    assert!(
        !cells[0].1.contains("<img"),
        "missing art must not render an image: {row}"
    );
    assert!(
        !row.contains(r#"src="""#),
        "missing art must not render an empty src: {row}"
    );
}

#[tokio::test]
async fn a_failing_service_saves_the_entry_and_reports_it() {
    // Arrange — every request errors. The entry must still be saved, and the
    // user must be told why it arrived bare.
    let stubs = crate::metadata_stub::MetadataStubs::failure().await;
    let app = spawn_app_against(stubs).await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — saved...
    assert!(
        response.status().is_success(),
        "a failing metadata service must not block the save"
    );
    let body = response.text().await.unwrap();
    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("Sabaton"),
        "the entry should be saved:\n{page}"
    );

    // ...and the user is told, via a signal patch
    assert!(
        body.contains("Could not look that up"),
        "no status signal was sent:\n{body}"
    );
    assert!(
        !body.contains(r#""year":0"#),
        "a failed lookup produced a zero year:\n{body}"
    );
}

#[tokio::test]
async fn a_miss_is_silent_because_it_is_not_a_failure() {
    // Arrange — nothing found. Different from an error, so no status message.
    let stubs = crate::metadata_stub::MetadataStubs::miss().await;
    let app = spawn_app_against(stubs).await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");
    let saved = response.status().is_success();
    let body = response.text().await.unwrap();

    // Assert — saved, and no scary status message
    assert!(saved);
    assert!(
        !body.contains("Could not look that up"),
        "a plain miss should not raise a status message:\n{body}"
    );
}

#[tokio::test]
async fn a_lookup_that_finds_nothing_still_saves_the_entry_bare() {
    // Arrange — a stubbed miss. No network, no live service.
    let stubs = crate::metadata_stub::MetadataStubs::miss().await;
    let app = spawn_app_against(stubs).await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&serde_json::json!({
            "artist": "Zzz Nonexistent Artist Zzz",
            "album": "Zzz Nonexistent Album Zzz",
            "note": ""
        }))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — saved, just without a cover or year.
    assert!(response.status().is_success());
    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();
    let (_, row) = row_with_id(table_body(&page), "rotation-entry-1");
    let cells = elements(row, "td");
    assert_eq!(
        cells.len(),
        6,
        "missing metadata must not remove cells: {row}"
    );
    assert!(cells[1].1.contains("Zzz Nonexistent Artist Zzz"));
    assert!(cells[2].1.contains("Zzz Nonexistent Album Zzz"));
    assert!(
        !cells[0].1.contains("<img"),
        "a missing cover rendered an image: {row}"
    );
    assert!(
        !row.contains(r#"src="""#),
        "a missing cover rendered an empty src: {row}"
    );
    assert!(
        !has_zero_year(cells[3].1),
        "a missing year rendered as zero: {row}"
    );
}

#[tokio::test]
async fn empty_state_is_shown_only_when_there_are_no_entries() {
    // Arrange
    let app = spawn_app().await;

    // Act — first look at an empty app
    let empty = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();

    // Assert — a visible six-column placeholder row inside the table body.
    let (tag, row) = row_with_id(table_body(&empty), "rotation-loading");
    assert!(
        has_class(tag, "rotation-empty"),
        "wrong empty-state class: {tag}"
    );
    assert!(!is_hidden(tag), "empty state should be visible: {tag}");
    let cells = elements(row, "td");
    assert_eq!(
        cells.len(),
        1,
        "empty state should have one spanning cell: {row}"
    );
    assert_eq!(attribute(cells[0].0, "colspan"), Some("6"));
    assert!(cells[0].1.contains("Nothing spinning yet"));

    // Act — add an entry, then look again
    app.api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");
    let filled = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();

    // Assert — the same placeholder is hidden when the table contains an entry.
    let (tag, row) = row_with_id(table_body(&filled), "rotation-loading");
    assert!(has_class(tag, "rotation-empty"));
    assert!(is_hidden(tag), "empty state should be hidden: {tag}");
    let cells = elements(row, "td");
    assert_eq!(cells.len(), 1);
    assert_eq!(attribute(cells[0].0, "colspan"), Some("6"));
    row_with_id(table_body(&filled), "rotation-entry-1");
}
#[tokio::test]
async fn sse_removes_the_empty_state_message() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let body = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.")
        .text()
        .await
        .unwrap();

    // The public widget stream still removes its consumer's placeholder.
    // The landing page's table-specific stream is checked separately below.
    assert!(
        body.contains("data: selector #rotation-loading"),
        "stream did not target the empty state:\n{body}"
    );
    assert!(
        body.contains("data: mode remove"),
        "stream did not remove the empty state:\n{body}"
    );
    // And the removal must come after the append, or the message is gone before it matters.
    let append_at = body.find("data: selector #rotation-list").unwrap();
    let remove_at = body.find("data: selector #rotation-loading").unwrap();
    assert!(
        append_at < remove_at,
        "removal should follow the append:\n{body}"
    );
}

#[tokio::test]
async fn sse_patch_is_a_single_data_elements_field() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let body = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.")
        .text()
        .await
        .unwrap();

    // Assert — the HTML for one entry must arrive as ONE `data: elements` field.
    // A multi-line template makes the encoder emit a `data: elements` prefix per
    // line, and Datastar then patches with fragments instead of the whole entry,
    // so only part of the entry lands in the DOM.
    let element_fields = body
        .lines()
        .filter(|line| line.starts_with("data: elements"))
        .count();
    assert_eq!(
        element_fields, 1,
        "expected exactly one `data: elements` field, got {element_fields}:\n{body}"
    );
}

#[tokio::test]
async fn every_posted_entry_appears_in_the_index_list() {
    // Arrange
    let app = spawn_app().await;

    // Act — post three different entries
    for (artist, album) in [
        ("Sabaton", "Attero Dominatus"),
        ("Alestorm", "Cocoon"),
        ("Alice Cooper", "Special Forces"),
    ] {
        let response = app
            .api_client
            .post(format!("{}/rotation", app.address))
            .json(&serde_json::json!({
                "artist": artist, "album": album, "note": ""
            }))
            .send()
            .await
            .expect("Failed to execute request.");
        assert!(response.status().is_success());
    }

    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();

    // All rows have stable, distinct IDs and retain insertion order.
    let body = table_body(&page);
    let rows: Vec<_> = elements(body, "tr")
        .into_iter()
        .filter(|(tag, _)| has_class(tag, "rotation-row"))
        .collect();
    assert_eq!(rows.len(), 3, "expected three rotation rows: {body}");
    for (index, (artist, album)) in [
        ("Sabaton", "Attero Dominatus"),
        ("Alestorm", "Cocoon"),
        ("Alice Cooper", "Special Forces"),
    ]
    .into_iter()
    .enumerate()
    {
        let expected_id = format!("rotation-entry-{}", index + 1);
        assert_eq!(attribute(rows[index].0, "id"), Some(expected_id.as_str()));
        let cells = elements(rows[index].1, "td");
        assert_eq!(cells.len(), 6, "expected six cells: {}", rows[index].1);
        assert!(cells[1].1.contains(artist));
        assert!(cells[2].1.contains(album));
    }
}

#[tokio::test]
async fn entry_text_is_escaped_in_the_sse_patch() {
    // Arrange
    let app = spawn_app().await;
    let mut body = payload();
    body["artist"] = serde_json::json!("<Artist>");
    body["album"] = serde_json::json!("Album & More");
    body["note"] = serde_json::json!("<img src=x onerror=alert(1)>");

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request.");
    let sse_body = response.text().await.unwrap();

    // Assert — the patch carries escaped text, never raw markup. The SSE path and the
    // server-rendered page must agree, or a stored payload runs for whoever loads the page.
    assert!(
        !sse_body.contains("<img src=x onerror=alert(1)>"),
        "raw payload leaked into the SSE patch: {sse_body}"
    );
    assert!(sse_body.contains("&lt;img src=x onerror=alert(1)&gt;"));

    // And the server-rendered page escapes it identically.
    let page = app
        .api_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request")
        .text()
        .await
        .unwrap();
    assert!(
        !page.contains("<img src=x onerror=alert(1)>"),
        "raw payload leaked into the page: {page}"
    );
    assert!(page.contains("&lt;img src=x onerror=alert(1)&gt;"));
    let (_, row) = row_with_id(table_body(&page), "rotation-entry-1");
    let cells = elements(row, "td");
    assert_eq!(cells.len(), 6);
    assert!(
        cells[1].1.contains("&lt;Artist&gt;"),
        "artist must be escaped: {row}"
    );
    assert!(
        cells[2].1.contains("Album &amp; More"),
        "album must be escaped: {row}"
    );
    assert!(
        cells[4].1.contains("&lt;img src=x onerror=alert(1)&gt;"),
        "note must be escaped: {row}"
    );
}
#[tokio::test]
async fn random_entry_with_no_entries_returns_404() {
    // Arrange — a fresh app has nothing in its rotation
    let app = spawn_app().await;

    // Assert — a 404, not a dropped connection.
    //
    // Uses the unauthenticated client on purpose: the blog island fetches this
    // route from a static site, so it must stay open. A regression that gates it
    // would show up here as a 401 instead.
    //
    // This is also the case that catches a missing emptiness guard: drawing a random
    // index from an empty range panics, so the request is killed mid-response and the
    // client sees a transport error rather than any status at all.
    let response = app
        .unauthenticated_client
        .get(format!("{}/rotation", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "an empty rotation should be a 404, not a dropped connection"
    );

    // And the server must still be healthy afterwards — a panic in a handler would
    // leave the app unable to serve the next request.
    let health = app
        .api_client
        .get(format!("{}/health_check", app.address))
        .send()
        .await
        .expect("app did not survive the empty-rotation request");
    assert!(health.status().is_success());
}

#[tokio::test]
async fn random_entry_is_one_of_the_entered_entries() {
    // Arrange — enter three entries
    let app = spawn_app().await;
    for artist in ["Sabaton", "Alestorm", "Alice Cooper"] {
        app.api_client
            .post(format!("{}/rotation", app.address))
            .json(&serde_json::json!({
                "artist": artist, "album": "X", "note": ""
            }))
            .send()
            .await
            .unwrap();
    }

    // Act — ask for one, sending no credentials: the island needs this open.
    let response = app
        .unauthenticated_client
        .get(format!("{}/rotation", app.address))
        .send()
        .await
        .unwrap();

    // Assert — 200 without credentials, because the island must stay readable.
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    assert!(
        body.contains("rotation-artist"),
        "expected a rendered entry, got: {body}"
    );
}

#[tokio::test]
async fn random_entry_also_clears_the_consumer_placeholder() {
    // The blog renders "Loading current rotation…" with id `rotation-loading`
    // inside `#rotation-list` and relies on this stream to take it away. Without
    // the removal the notice sits underneath the widget forever, which is exactly
    // the bug this contract exists to prevent.
    let app = spawn_app().await;
    app.api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .unwrap();

    let body = app
        .unauthenticated_client
        .get(format!("{}/rotation", app.address))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        body.contains("data: selector #rotation-loading"),
        "stream did not clear the consumer placeholder:\n{body}"
    );
    assert!(
        body.contains("data: mode remove"),
        "placeholder removal is missing its mode:\n{body}"
    );
    // The removal must follow the append, or the placeholder vanishes before the
    // entry it is making room for has arrived.
    let append_at = body.find("data: selector #rotation-list").unwrap();
    let remove_at = body.find("data: selector #rotation-loading").unwrap();
    assert!(
        append_at < remove_at,
        "removal should follow the append:\n{body}"
    );
}

#[tokio::test]
async fn posting_without_credentials_is_rejected() {
    // The blog island can read, but only the owner should be able to write.
    // A POST with no Authorization header must get a 401 — and specifically a
    // 401 carrying WWW-Authenticate, which is what makes a browser render its
    // native login box rather than showing a blank error.
    let app = spawn_app().await;

    let response = app
        .unauthenticated_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "an unauthenticated POST must not reach the handler"
    );
    let challenge = response
        .headers()
        .get("www-authenticate")
        .expect("401 must carry WWW-Authenticate or no browser login box appears")
        .to_str()
        .unwrap();
    assert!(
        challenge.starts_with("Basic"),
        "expected a Basic challenge, got: {challenge}"
    );
}

#[tokio::test]
async fn posting_with_wrong_credentials_is_rejected() {
    // Presence of a header is not enough — the comparison must actually compare.
    // This is the test that catches an implementation that decodes and forgets
    // to check, which would accept any well-formed credentials.
    let app = spawn_app().await;

    let encoded = base64::engine::general_purpose::STANDARD.encode("test:wrongpass");
    let response = app
        .unauthenticated_client
        .post(format!("{}/rotation", app.address))
        .header("Authorization", format!("Basic {encoded}"))
        .json(&payload())
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "wrong credentials must be rejected, got {}",
        response.status()
    );
}

#[tokio::test]
async fn the_index_page_stays_open() {
    // The form lives on `/` and is fetched before any login happens. It is not
    // gated — the browser will challenge on the first POST instead — so a GET
    // with no credentials must still render the page.
    let app = spawn_app().await;

    let response = app
        .unauthenticated_client
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the form page should load before authentication"
    );
}

// --- SSE transport contract (actix-sse migration) ---
//
// The stream is a wire format first and a Datastar payload second. These tests
// pin the transport: headers, framing, event order and payload shapes, plus the
// bounded lifetime the finite event lists imply.

#[tokio::test]
async fn post_rotation_streams_a_well_framed_finite_sse_response() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — transport headers, framing, and a stream that ends on its own
    assert!(response.status().is_success());
    assert_sse_headers(&response);
    let body = sse_body(response).await;

    // Three events: the append, the placeholder removal, the signal clear.
    let events = assert_sse_framing(&body, 3);
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
    assert_eq!(
        events[2].event.as_deref(),
        Some("datastar-patch-signals"),
        "the third event must patch signals:\n{body}"
    );

    // ...in that order: append, then clear the empty state, then reset the form.
    assert_eq!(
        events[0].dataline("selector"),
        Some("#rotation-list"),
        "the append must target the list:\n{body}"
    );
    assert_eq!(
        events[0].dataline("mode"),
        Some("append"),
        "the append must be an append:\n{body}"
    );
    assert_eq!(
        events[1].dataline("selector"),
        Some("#rotation-loading"),
        "the removal must target the empty-state placeholder:\n{body}"
    );
    assert_eq!(
        events[1].dataline("mode"),
        Some("remove"),
        "the placeholder event must be a removal:\n{body}"
    );

    // The element patch carries the rendered entry as ONE dataline; the signal
    // patch carries one JSON object.
    assert_eq!(
        events[0]
            .data
            .iter()
            .filter(|line| line.starts_with("elements "))
            .count(),
        1,
        "the entry must arrive as a single `elements` dataline:\n{body}"
    );
    assert_eq!(
        events[2]
            .data
            .iter()
            .filter(|line| line.starts_with("signals "))
            .count(),
        1,
        "the reset must arrive as a single `signals` dataline:\n{body}"
    );
    let cleared: serde_json::Value = events[2]
        .dataline("signals")
        .expect("the signals patch was expected")
        .parse()
        .expect("the signals dataline must be a single JSON value");
    let html = events[0].dataline("elements").expect("entry HTML payload");
    assert!(html.contains("rotation-artist"));
    assert!(html.contains("Sabaton"));
    assert!(events[1].dataline("elements").is_none());
    assert!(cleared.is_object(), "signals must be a JSON object");
    for signal in ["artist", "album", "note", "metadata_status"] {
        assert_eq!(cleared[signal].as_str(), Some(""), "reset {signal}: {body}");
    }
}

#[tokio::test]
async fn get_rotation_streams_a_well_framed_finite_sse_response() {
    // Arrange — give the random picker something to draw
    let app = spawn_app().await;
    let seed = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .expect("Failed to execute request.");
    assert!(seed.status().is_success());
    sse_body(seed).await;

    // Act — the island fetches this unauthenticated, on purpose
    let response = app
        .unauthenticated_client
        .get(format!("{}/rotation", app.address))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    assert_sse_headers(&response);
    let body = sse_body(response).await;

    // Two events: the append, then the placeholder removal.
    let events = assert_sse_framing(&body, 2);
    assert_eq!(events[1].event.as_deref(), Some("datastar-patch-elements"));
    let html = events[0].dataline("elements").expect("entry HTML payload");
    assert!(html.contains("rotation-artist"));
    assert!(html.contains("Sabaton"));
    assert!(events[1].dataline("elements").is_none());
    assert_eq!(
        events[0].event.as_deref(),
        Some("datastar-patch-elements"),
        "the first event must patch elements:\n{body}"
    );
    assert_eq!(
        events[0].dataline("selector"),
        Some("#rotation-list"),
        "the append must target the list:\n{body}"
    );
    assert_eq!(
        events[0].dataline("mode"),
        Some("append"),
        "the append must be an append:\n{body}"
    );
    assert_eq!(
        events[1].dataline("selector"),
        Some("#rotation-loading"),
        "the removal must target the placeholder:\n{body}"
    );
    assert_eq!(
        events[1].dataline("mode"),
        Some("remove"),
        "the placeholder event must be a removal:\n{body}"
    );
}

#[tokio::test]
async fn post_rotation_lookup_failure_appends_a_fourth_status_signal_patch() {
    // Arrange — every request to the metadata services errors, so the entry is
    // saved bare and the user must be told why.
    let stubs = crate::metadata_stub::MetadataStubs::failure().await;
    let app = spawn_app_against(stubs).await;

    // Act
    let response = app
        .api_client
        .post(format!("{}/rotation", app.address))
        .json(&payload())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert — the same three events as a clean post, plus a fourth patch
    // carrying the status signal.
    assert!(response.status().is_success());
    assert_sse_headers(&response);
    let body = sse_body(response).await;

    let events = assert_sse_framing(&body, 4);
    assert_eq!(
        events
            .iter()
            .map(|event| event.event.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("datastar-patch-elements"),
            Some("datastar-patch-elements"),
            Some("datastar-patch-signals"),
            Some("datastar-patch-signals"),
        ],
        "append, remove, reset, then lookup status:\n{body}"
    );
    assert_eq!(events[0].dataline("selector"), Some("#rotation-list"));
    assert_eq!(events[0].dataline("mode"), Some("append"));
    assert!(events[0].dataline("elements").unwrap().contains("Sabaton"));
    assert_eq!(events[1].dataline("selector"), Some("#rotation-loading"));
    assert_eq!(events[1].dataline("mode"), Some("remove"));
    let cleared: serde_json::Value = events[2].dataline("signals").unwrap().parse().unwrap();
    assert_eq!(cleared["metadata_status"].as_str(), Some(""));
    let status = &events[3];
    assert_eq!(
        status.event.as_deref(),
        Some("datastar-patch-signals"),
        "the extra event must patch signals:\n{body}"
    );
    let signals: serde_json::Value = status
        .dataline("signals")
        .expect("the status patch must carry a signals dataline")
        .parse()
        .expect("the status signals must be a single JSON value");
    assert_eq!(
        signals["metadata_status"], "Could not look that up - saved without a cover or year",
        "the status patch must carry the lookup-failure message:\n{body}"
    );
}

#[tokio::test]
async fn invalid_signals_are_rejected_without_saving_an_entry() {
    let app = spawn_app().await;

    for body in [
        r#"{"artist":"Sabaton""#,
        r#"{"artist":"Sabaton","album":"Attero Dominatus"}"#,
    ] {
        let response = app
            .api_client
            .post(format!("{}/rotation", app.address))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .expect("Failed to execute request.");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "body: {body}");
    }

    let response = app
        .unauthenticated_client
        .get(format!("{}/rotation", app.address))
        .send()
        .await
        .expect("Failed to execute request.");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// The landing page consumes a different fragment from the public blog widget.
// Keep its transport, payload and initial-render contracts tied together.
#[tokio::test]
async fn table_post_streams_one_shared_row_then_removes_placeholder_and_resets() {
    let stubs =
        crate::metadata_stub::MetadataStubs::full_hit("b4974b0b-305e-4f98-b16d-04112f3cced2").await;
    let app = spawn_app_against(stubs).await;
    let before = jiff::ZonedDateTime::now().date().to_string();
    let response = app
        .api_client
        .post(format!("{}/rotation?view=table", app.address))
        .json(&payload())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .unwrap();
    let after = jiff::ZonedDateTime::now().date().to_string();
    assert!(response.status().is_success());
    assert_sse_headers(&response);
    let body = sse_body(response).await;
    let events = assert_sse_framing(&body, 3);
    assert_eq!(
        events
            .iter()
            .map(|e| e.event.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("datastar-patch-elements"),
            Some("datastar-patch-elements"),
            Some("datastar-patch-signals")
        ],
        "append, remove, then reset: {body}"
    );
    assert_eq!(events[0].dataline("selector"), Some("#rotation-table-body"));
    assert_eq!(events[0].dataline("mode"), Some("append"));
    assert_eq!(
        events[0]
            .data
            .iter()
            .filter(|line| line.starts_with("elements "))
            .count(),
        1
    );
    let html = events[0].dataline("elements").expect("one row payload");
    let rows = elements(html, "tr");
    assert_eq!(rows.len(), 1, "must append exactly one table row: {html}");
    assert_eq!(attribute(rows[0].0, "id"), Some("rotation-entry-1"));
    assert!(has_class(rows[0].0, "rotation-row"));
    assert!(
        html.trim_start().starts_with("<tr") && html.trim_end().ends_with("</tr>"),
        "payload must be a row, not a wrapper: {html}"
    );
    let cells = elements(rows[0].1, "td");
    assert_eq!(cells.len(), 6, "six table cells: {html}");
    assert!(cells[0].1.contains("front-250.jpg"));
    assert!(cells[1].1.contains("Sabaton"));
    assert!(cells[2].1.contains("Attero Dominatus"));
    assert!(cells[3].1.contains("2006"));
    assert!(
        cells[4]
            .1
            .contains("A co-worker turned me on to Sabaton in early 2022.")
    );
    assert!(
        cells[5].1.contains(&before) || cells[5].1.contains(&after),
        "missing listened date: {html}"
    );
    assert_eq!(events[1].dataline("selector"), Some("#rotation-loading"));
    assert_eq!(events[1].dataline("mode"), Some("remove"));
    assert!(events[1].dataline("elements").is_none());
    let signals: serde_json::Value = events[2].dataline("signals").unwrap().parse().unwrap();
    for key in ["artist", "album", "note", "metadata_status"] {
        assert_eq!(signals[key].as_str(), Some(""), "reset {key}: {body}");
    }

    // The server-rendered page must carry the same row and field values.
    let page = app
        .unauthenticated_client
        .get(&app.address)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let (tag, contents) = row_with_id(table_body(&page), "rotation-entry-1");
    assert!(has_class(tag, "rotation-row"));
    let persisted = elements(contents, "td");
    assert_eq!(persisted.len(), 6);
    for (cell, expected) in persisted.iter().zip([
        "front-250.jpg",
        "Sabaton",
        "Attero Dominatus",
        "2006",
        "A co-worker turned me on to Sabaton in early 2022.",
    ]) {
        assert!(
            cell.1.contains(expected),
            "missing {expected} from {contents}"
        );
    }
    assert!(persisted[5].1.contains(&before) || persisted[5].1.contains(&after));
}

#[tokio::test]
async fn table_post_escapes_fields_and_missing_metadata_on_both_render_paths() {
    let app = spawn_app().await;
    let before = jiff::ZonedDateTime::now().date().to_string();
    let input = serde_json::json!({
        "artist": "<Artist>", "album": "Album & More", "note": "<img src=x onerror=alert(1)>"
    });
    let response = app
        .api_client
        .post(format!("{}/rotation?view=table", app.address))
        .json(&input)
        .send()
        .await
        .unwrap();
    let after = jiff::ZonedDateTime::now().date().to_string();
    assert!(response.status().is_success());
    let body = sse_body(response).await;
    let events = assert_sse_framing(&body, 3);
    let html = events[0].dataline("elements").expect("row payload");
    assert_eq!(events[0].dataline("selector"), Some("#rotation-table-body"));
    let (_, sse_row) = row_with_id(html, "rotation-entry-1");
    let page = app
        .unauthenticated_client
        .get(&app.address)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let (_, page_row) = row_with_id(table_body(&page), "rotation-entry-1");
    for row in [sse_row, page_row] {
        let cells = elements(row, "td");
        assert_eq!(
            cells.len(),
            6,
            "missing metadata must retain all columns: {row}"
        );
        assert!(
            !cells[0].1.contains("<img"),
            "a missing cover should not create an image: {row}"
        );
        assert!(!cells[0].1.contains(r#"src="""#));
        assert!(
            cells[1].1.contains("&lt;Artist&gt;"),
            "artist must be escaped: {row}"
        );
        assert!(
            cells[2].1.contains("Album &amp; More"),
            "album must be escaped: {row}"
        );
        assert!(
            !has_zero_year(cells[3].1),
            "a missing year must not be zero: {row}"
        );
        assert!(
            cells[4].1.contains("&lt;img src=x onerror=alert(1)&gt;"),
            "note must be escaped: {row}"
        );
        assert!(
            !row.contains("<img src=x onerror=alert(1)>"),
            "raw markup leaked: {row}"
        );
        assert!(
            cells[5].1.contains(&before) || cells[5].1.contains(&after),
            "missing listened date: {row}"
        );
    }
}

#[tokio::test]
async fn table_post_failure_saves_row_resets_signals_and_reports_lookup_error() {
    let stubs = crate::metadata_stub::MetadataStubs::failure().await;
    let app = spawn_app_against(stubs).await;
    let response = app
        .api_client
        .post(format!("{}/rotation?view=table", app.address))
        .json(&payload())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_sse_headers(&response);
    let body = sse_body(response).await;
    let events = assert_sse_framing(&body, 4);
    assert_eq!(
        events
            .iter()
            .map(|e| e.event.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("datastar-patch-elements"),
            Some("datastar-patch-elements"),
            Some("datastar-patch-signals"),
            Some("datastar-patch-signals")
        ],
        "append, remove, reset, then warning: {body}"
    );
    assert_eq!(events[0].dataline("selector"), Some("#rotation-table-body"));
    assert_eq!(events[0].dataline("mode"), Some("append"));
    let (tag, row) = row_with_id(events[0].dataline("elements").unwrap(), "rotation-entry-1");
    assert!(has_class(tag, "rotation-row"));
    let cells = elements(row, "td");
    assert_eq!(cells.len(), 6);
    assert!(cells[1].1.contains("Sabaton"));
    assert!(!cells[0].1.contains("<img"));
    assert!(!has_zero_year(cells[3].1));
    assert_eq!(events[1].dataline("selector"), Some("#rotation-loading"));
    assert_eq!(events[1].dataline("mode"), Some("remove"));
    let reset: serde_json::Value = events[2].dataline("signals").unwrap().parse().unwrap();
    for key in ["artist", "album", "note", "metadata_status"] {
        assert_eq!(reset[key].as_str(), Some(""), "reset {key}: {body}");
    }
    let warning: serde_json::Value = events[3].dataline("signals").unwrap().parse().unwrap();
    assert_eq!(
        warning["metadata_status"],
        "Could not look that up - saved without a cover or year"
    );
    let page = app
        .unauthenticated_client
        .get(&app.address)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let (_, persisted) = row_with_id(table_body(&page), "rotation-entry-1");
    assert!(
        persisted.contains("Sabaton"),
        "failed lookup must not lose the entry: {page}"
    );
}
