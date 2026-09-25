# GIFs (KLIPY)

#engineering #feature

A GIF picker in the composer, plus inline playback of GIFs in the feed. Both follow the official Bluesky apps exactly, so a GIF posted from Smooblue animates in bsky.app and the mobile apps, and theirs animate in Smooblue.

## Why KLIPY, not Tenor

Google shut down the public Tenor API. It stopped issuing new keys on 2026-01-13, and requests stopped working after 2026-06-30. The Bluesky apps moved their picker to **KLIPY**, whose v2 API is Tenor-compatible: same `search` / `featured` endpoints, same response shape, host `api.klipy.com`. Smooblue uses KLIPY too. Tenor GIFs posted before the switch are still everywhere in timelines, so the feed plays both.

## How Bluesky does GIFs

There is no GIF blob type. A GIF is an `app.bsky.embed.external` link to the GIF CDN, which the official apps play inline. The embed fields come from social-app `resolveGif`, `createGIFDescription`, `parseKlipyGif` and `parseTenorGif`:

| Field | Value |
| --- | --- |
| uri (KLIPY) | `https://static.klipy.com/ii/…/<slug>.gif?hh=<h>&ww=<w>&mp4=<slug>&webm=<slug>`. The video slugs let web clients play it as video. |
| uri (Tenor, legacy) | `https://media.tenor.com/<id>AAAAC/<name>.gif?hh=<h>&ww=<w>` |
| title | the GIF's description |
| description | `ALT: <vendor description>`, or `Alt: <user alt>` when the user wrote alt text |
| thumb | the still preview frame |

## In Smooblue

| Piece | Where |
| --- | --- |
| Client | `crates/smooblue-app/src/gifs.rs`: `search` (featured when the query is empty, `pos` paging), `Gif::embed_uri`, `gif_description`, `gif_embed_dims`. All unit tested. |
| Picker | `GifPicker` in `components/compose.rs`, opened by the **GIF** button. Trending until you type, then a 300 ms debounced search, and **More** pages on. It shows "Search KLIPY" / "Powered by KLIPY", which is KLIPY's attribution requirement for a production key. |
| Posting | `SelectedGif::as_card()` is the first post's link card, and wins over an auto-detected URL card. A GIF owns the media slot, so it's exclusive with images and video. It has an optional alt field. Drafts save it. |
| Feed | `embed.rs` `GifEmbed`: any KLIPY or Tenor GIF link embed renders as an animated `<img>` sized from `ww`/`hh`, with a GIF badge. Click opens the lightbox. |

## The API key

`SMOOBLUE_KLIPY_KEY` comes from [partner.klipy.com](https://partner.klipy.com) → **Add Platform**. Test keys are capped at 100 calls/hour. Request production access in the Partner Panel once the attribution is in, as it is here. It's a client-side key and is **never committed**:

- **Release builds:** baked in at compile time (`option_env!`) from the `SMOOBLUE_KLIPY_KEY` Actions secret (`release.yml`).
- **Dev:** export `SMOOBLUE_KLIPY_KEY`. The runtime env var also overrides the baked-in key.
- **No key:** no GIF button. GIFs in the feed still play.
