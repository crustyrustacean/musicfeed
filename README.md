# MusicFeed

A small service that keeps a "currently spinning" widget on my music blog up to date.

Record an album once — artist, title, an optional note — and it appears in the widget on
[Crusty Metallian](https://crusty-metallian.net)'s front page. Cover art and release year are
looked up automatically, and a background pass retries any lookup that failed. The point is to
tell visitors about music they might not otherwise hear about, without me editing the site by hand.

## Try it

- **See it:** [Crusty Metallian](https://crusty-metallian.net) — the widget is on the front page.
- **The service:** [musicfeed.crusty-metallian.net](https://musicfeed.crusty-metallian.net)

The service is the form for adding entries and the API the blog widget reads. Writing is protected
by HTTP Basic Authentication and I'm the only authorized user, so visitors only ever read. There is
nothing for anyone else to install or run.

## How it works

1. Submit artist, album, and an optional note through the form.
2. The service asks [MusicBrainz](https://musicbrainz.org/) for the release year and the
   [Cover Art Archive](https://coverartarchive.org/) for the artwork.
3. The entry is saved to SQLite — whether or not the lookup succeeded. A miss never costs you the
   entry; it just arrives without a cover, and a healer pass retries later.
4. The blog fetches `GET /rotation` and streams the current pick into the widget with
   [Datastar](https://data-star.dev/).

## Endpoints

| Method | Path            | Notes                                                        |
| ------ | --------------- | ------------------------------------------------------------ |
| `GET`  | `/`             | The entry form and the full rotation list                     |
| `GET`  | `/rotation`     | Server-sent events: one randomly chosen entry as a Datastar patch |
| `POST` | `/rotation`     | Add an entry (Basic auth). Streams the patch back             |
| `GET`  | `/health_check` | Liveness                                                       |

`GET /rotation` answers `404` when the rotation is empty.

### The stream contract

Both `GET /rotation` and `POST /rotation` answer with server-sent events carrying Datastar
`datastar-patch-elements` messages, in this order:

1. **append** a rendered entry to `#rotation-list`
2. **remove** `#rotation-loading`

The second message is what lets a consumer show a placeholder — a loading notice, an empty-state
message — inside the list without it surviving underneath the widget. Put your placeholder in the
list with `id="rotation-loading"` and the stream will clear it once there is an entry to show.
If you have no such element, nothing happens: removing a missing node is a no-op.

```html
<section id="rotation-list" data-init="@get('https://musicfeed.crusty-metallian.net/rotation')">
  <p id="rotation-loading">Loading current rotation…</p>
</section>
```

The entries themselves are appended, never replaced, so a consumer that wants exactly one visible
entry should keep the last appended `.rotation-widget` and drop any earlier ones.

## Built with

- Rust and [Axum](https://github.com/tokio-rs/axum)
- [Tera](https://github.com/Keats/tera) templates and [Datastar](https://data-star.dev/) for the
  interactive form and the live patch
- SQLite via [sqlx](https://github.com/launchbadge/sqlx), migrations embedded in the binary
- MusicBrainz and the Cover Art Archive for album metadata

This grew out of my [Axum-Tera-Datastar starter](https://github.com/crustyrustacean/axum-tera-datastar).

## Running it

Configuration is layered: `configuration/base.toml`, then an environment file, then `APP_*`
environment variables.

```bash
cargo run                # http://127.0.0.1:8000, using configuration/local.toml
cargo test               # 66 tests, no network required
```

The default local credentials are `test` / `test`. Set real ones for anything reachable:

```bash
export APP_BASICAUTH__USERNAME=...
export APP_BASICAUTH__PASSWORD=...
```

Container build and deploy:

```bash
docker build -t musicfeed .
docker run -p 8000:8000 -e APP_ENVIRONMENT=production musicfeed
```

`APP_ENVIRONMENT=production` reads `configuration/production.toml` and expects the SQLite volume at
`/data`. Migrations are embedded with `sqlx::migrate!`, so changing a migration needs a rebuild
rather than a new file in a running image.

Because browsers are the only clients CORS can restrain, `[cors] allowed_origins` lists the blog's
origins — see `configuration/base.toml`. That is a convenience for the widget, not authentication:
`POST /rotation` is protected by Basic auth, which is what actually keeps it closed.

## History

Originally built for [ZTM Build Fest 2026](https://github.com/zero-to-mastery/ZTM-Build-Fest); the
submission lives there and this repository is where it continues.

I use AI to help brainstorm and guide development, and my own harness,
[Rho Code](https://rho-code.dev), as a research assistant, pair programmer, and reviewer.

## Licence

MIT — see [LICENSE](LICENSE).
