// tests/api/index.rs

use crate::helpers::spawn_app;

/// Find non-nested elements of a given tag without depending on indentation or
/// attribute order. These tests only use it for the table's structural elements.
pub(super) fn elements<'a>(html: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = html;
    let mut found = Vec::new();
    while let Some(start) = rest.find(&prefix) {
        let candidate = &rest[start..];
        let boundary = candidate[prefix.len()..].chars().next();
        if !boundary.is_some_and(|c| c.is_whitespace() || c == '>') {
            rest = &candidate[prefix.len()..];
            continue;
        }
        let end = candidate.find('>').expect("opening tag must end");
        let content = &candidate[end + 1..];
        let closing = content.find(&close).expect("element must close");
        found.push((&candidate[..=end], &content[..closing]));
        rest = &content[closing + close.len()..];
    }
    found
}

/// Read a quoted attribute; allow whitespace around `=` and unrelated attributes.
pub(super) fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for (pos, _) in tag.match_indices(name) {
        if pos == 0 || !tag.as_bytes()[pos - 1].is_ascii_whitespace() {
            continue;
        }
        let after = tag[pos + name.len()..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &after[quote.len_utf8()..];
        return value.find(quote).map(|end| &value[..end]);
    }
    None
}

pub(super) fn has_class(tag: &str, class: &str) -> bool {
    attribute(tag, "class").is_some_and(|value| value.split_whitespace().any(|c| c == class))
}

pub(super) fn is_hidden(tag: &str) -> bool {
    tag.trim_end_matches('>')
        .split_whitespace()
        .any(|part| part == "hidden" || part.starts_with("hidden="))
}

/// Detect a rendered zero year in text, not digits in an unrelated HTML attribute.
pub(super) fn has_zero_year(cell: &str) -> bool {
    cell.split('<').enumerate().any(|(index, part)| {
        let text = if index == 0 {
            part
        } else if let Some((_, text)) = part.split_once('>') {
            text
        } else {
            return false;
        };
        matches!(text.trim(), "0" | "(0)")
    })
}

pub(super) fn table_body(page: &str) -> &str {
    let bodies = elements(page, "tbody");
    let matches: Vec<_> = bodies
        .iter()
        .filter(|(tag, _)| attribute(tag, "id") == Some("rotation-table-body"))
        .collect();
    assert_eq!(matches.len(), 1, "expected one rotation table body: {page}");
    matches[0].1
}

pub(super) fn row_with_id<'a>(html: &'a str, id: &str) -> (&'a str, &'a str) {
    let rows = elements(html, "tr");
    let matches: Vec<_> = rows
        .into_iter()
        .filter(|(tag, _)| attribute(tag, "id") == Some(id))
        .collect();
    assert_eq!(matches.len(), 1, "expected one row with id {id}: {html}");
    matches[0]
}

#[tokio::test]
async fn index_page_returns_200() {
    let app = spawn_app().await;
    let response = reqwest::Client::new()
        .get(&app.address)
        .send()
        .await
        .expect("Failed to execute request.");
    assert!(response.status().is_success());
}

#[tokio::test]
async fn index_has_semantic_rotation_table_with_six_ordered_columns() {
    let app = spawn_app().await;
    let response = app
        .unauthenticated_client
        .get(&app.address)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let page = response.text().await.unwrap();

    let tables: Vec<_> = elements(&page, "table")
        .into_iter()
        .filter(|(_, contents)| {
            elements(contents, "tbody")
                .iter()
                .any(|(tag, _)| attribute(tag, "id") == Some("rotation-table-body"))
        })
        .collect();
    assert_eq!(
        tables.len(),
        1,
        "expected a semantic rotation table: {page}"
    );
    let table = tables[0].1;
    let heads = elements(table, "thead");
    assert_eq!(heads.len(), 1, "expected a table head: {table}");
    let header_rows = elements(heads[0].1, "tr");
    assert_eq!(header_rows.len(), 1, "expected one header row: {table}");
    let headings: Vec<_> = elements(header_rows[0].1, "th")
        .into_iter()
        .map(|(_, text)| text.trim())
        .collect();
    assert_eq!(
        headings,
        ["Cover", "Artist", "Album", "Year", "Note", "Listened"]
    );
    assert_eq!(elements(table, "tbody").len(), 1);
    table_body(&page);
    let forms = elements(&page, "form");
    let form = forms
        .iter()
        .find(|(tag, _)| attribute(tag, "id") == Some("rotation-form"))
        .expect("rotation form must exist");
    assert!(
        attribute(form.0, "data-on:submit__prevent")
            .is_some_and(|action| action.contains("/rotation?view=table")),
        "form must request table updates: {}",
        form.0
    );
}
