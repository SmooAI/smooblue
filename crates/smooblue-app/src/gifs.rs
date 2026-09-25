//! GIF search (KLIPY) for the composer's GIF picker, plus the helpers
//! that post and render GIFs the way the official Bluesky apps do.
//!
//! Google shut the public Tenor API down (no new keys from 2026-01-13,
//! off 2026-06-30); the Bluesky apps moved their picker to KLIPY, whose
//! v2 API is Tenor-compatible (same endpoints and response shape). So
//! does Smooblue.
//!
//! Bluesky has no GIF blob type. The official apps post a GIF as an
//! `app.bsky.embed.external` link to the GIF CDN with its size in the
//! query — `https://static.klipy.com/ii/…/<slug>.gif?hh=<h>&ww=<w>&mp4=<slug>&webm=<slug>`
//! for KLIPY (the video slugs let web clients play it as video), and
//! `https://media.tenor.com/<id>AAAAC/<name>.gif?hh=<h>&ww=<w>` for the
//! older Tenor GIFs still all over timelines — with the GIF's
//! description as the title, `ALT: <description>` as the description,
//! and the still preview frame as the thumb. [`Gif::embed_uri`] /
//! [`gif_description`] reproduce that exactly (social-app `resolveGif`
//! / `createGIFDescription`), so a GIF posted from Smooblue animates in
//! bsky.app and the mobile apps, and [`gif_embed_dims`] lets our feed
//! play theirs.
//!
//! The KLIPY API key is baked in at build time (`SMOOBLUE_KLIPY_KEY`,
//! a release-workflow secret — never committed) and can be overridden
//! at runtime with the same env var. No key → no GIF button.

use serde::Deserialize;

const GIF_API: &str = "https://api.klipy.com/v2";
/// Identifies this integration to KLIPY (their `client_key` param).
const CLIENT_KEY: &str = "smooblue";
/// Results per page in the picker grid.
pub const PAGE: u32 = 24;

/// The KLIPY API key: runtime env first (dev), then the one compiled
/// into release builds.
pub fn gif_api_key() -> Option<String> {
    std::env::var("SMOOBLUE_KLIPY_KEY")
        .ok()
        .or_else(|| option_env!("SMOOBLUE_KLIPY_KEY").map(str::to_string))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// One GIF, flattened to what the picker and the post need.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Gif {
    pub id: String,
    /// Full GIF on the provider CDN (the embed link, before its query).
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// Small animated GIF for the picker grid.
    pub tiny_url: String,
    /// Still frame — the link-card thumb, like the official apps.
    pub preview_url: String,
    /// The provider's description ("Cat Typing GIF"), used as alt text.
    pub description: String,
    /// KLIPY names each format's file differently; web clients need the
    /// mp4 / webm file stems to play the GIF as video, so they ride in
    /// the embed URL (as the Bluesky apps do).
    #[serde(default)]
    pub mp4_slug: Option<String>,
    #[serde(default)]
    pub webm_slug: Option<String>,
}

impl Gif {
    /// The external-embed URI, in the exact shape the Bluesky apps
    /// parse as a playable GIF.
    pub fn embed_uri(&self) -> String {
        let mut uri = format!("{}?hh={}&ww={}", self.url, self.height, self.width);
        let is_klipy = url::Url::parse(&self.url)
            .ok()
            .is_some_and(|u| u.host_str() == Some("static.klipy.com"));
        if is_klipy {
            if let Some(s) = &self.mp4_slug {
                uri.push_str(&format!("&mp4={s}"));
            }
            if let Some(s) = &self.webm_slug {
                uri.push_str(&format!("&webm={s}"));
            }
        }
        uri
    }
}

/// `ALT: <provider description>` — or `Alt: <yours>` when the user wrote
/// alt text. The two prefixes are how the official apps tell a vendor
/// description from a user's (social-app `createGIFDescription`).
pub fn gif_description(vendor_description: &str, user_alt: &str) -> String {
    let user_alt = user_alt.trim();
    if user_alt.is_empty() {
        format!("ALT: {vendor_description}")
    } else {
        format!("Alt: {user_alt}")
    }
}

/// `(width, height)` if `uri` is a GIF link as the Bluesky apps post
/// them — KLIPY (`static.klipy.com/ii/…`) or Tenor (`media.tenor.com/
/// <id>AAAAC/<name>.gif`), each with `hh` / `ww`. How the feed decides
/// to play an external embed inline. Mirrors social-app
/// `parseKlipyGif` / `parseTenorGif`.
pub fn gif_embed_dims(uri: &str) -> Option<(u32, u32)> {
    let url = url::Url::parse(uri).ok()?;
    let recognized = match url.host_str()? {
        "static.klipy.com" => url.path().starts_with("/ii/"),
        "media.tenor.com" => {
            let mut segs = url.path_segments()?;
            let id = segs.next()?;
            let file = segs.next()?;
            id.contains("AAAAC") && file.to_ascii_lowercase().ends_with(".gif")
        }
        _ => false,
    };
    if !recognized {
        return None;
    }
    let param = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .and_then(|(_, v)| v.parse::<u32>().ok())
            .filter(|n| *n > 0)
    };
    Some((param("ww")?, param("hh")?))
}

/// The file stem of a media URL (`…/abc-123.mp4` → `abc-123`).
fn file_slug(url: &str) -> Option<String> {
    let file = url::Url::parse(url)
        .ok()?
        .path_segments()?
        .next_back()?
        .to_string();
    let dot = file.rfind('.')?;
    (dot > 0).then(|| file[..dot].to_string())
}

// ── KLIPY (Tenor-compatible) API ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TenorResponse {
    #[serde(default)]
    results: Vec<TenorResult>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
struct TenorResult {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content_description: String,
    media_formats: std::collections::HashMap<String, TenorMedia>,
}

#[derive(Deserialize)]
struct TenorMedia {
    url: String,
    #[serde(default)]
    dims: Vec<u32>,
}

/// A page of results plus Tenor's cursor for the next one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GifPage {
    pub gifs: Vec<Gif>,
    pub next: Option<String>,
}

fn parse_page(body: &str) -> Result<GifPage, String> {
    let resp: TenorResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let gifs = resp
        .results
        .into_iter()
        .filter_map(|r| {
            let gif = r.media_formats.get("gif")?;
            let (w, h) = match gif.dims.as_slice() {
                [w, h, ..] if *w > 0 && *h > 0 => (*w, *h),
                _ => return None,
            };
            let tiny = r
                .media_formats
                .get("tinygif")
                .map(|m| m.url.clone())
                .unwrap_or_else(|| gif.url.clone());
            let preview = r
                .media_formats
                .get("preview")
                .map(|m| m.url.clone())
                .unwrap_or_else(|| tiny.clone());
            let slug_of = |fmt: &str| r.media_formats.get(fmt).and_then(|m| file_slug(&m.url));
            let (mp4_slug, webm_slug) = (slug_of("mp4"), slug_of("webm"));
            let description = if r.content_description.trim().is_empty() {
                r.title
            } else {
                r.content_description
            };
            Some(Gif {
                id: r.id,
                url: gif.url.clone(),
                width: w,
                height: h,
                tiny_url: tiny,
                preview_url: preview,
                description,
                mp4_slug,
                webm_slug,
            })
        })
        .collect();
    Ok(GifPage {
        gifs,
        next: resp.next.filter(|n| !n.is_empty() && n != "0"),
    })
}

/// Search KLIPY, or its featured/trending set when `query` is blank.
/// `pos` is the previous page's `next`.
pub async fn search(
    http: &reqwest::Client,
    key: &str,
    query: &str,
    pos: Option<&str>,
) -> Result<GifPage, String> {
    let q = query.trim();
    let endpoint = if q.is_empty() { "featured" } else { "search" };
    let mut url = url::Url::parse(&format!("{GIF_API}/{endpoint}")).map_err(|e| e.to_string())?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("key", key)
            .append_pair("client_key", CLIENT_KEY)
            .append_pair("limit", &PAGE.to_string())
            .append_pair("media_filter", "gif,tinygif,preview,mp4,webm")
            .append_pair("contentfilter", "medium");
        if !q.is_empty() {
            qp.append_pair("q", q);
        }
        if let Some(p) = pos {
            qp.append_pair("pos", p);
        }
    }
    let resp = http.get(url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("KLIPY returned {status}"));
    }
    parse_page(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KLIPY: &str = r#"{
        "results": [
            {
                "id": "123",
                "title": "cat",
                "content_description": "Cat Typing GIF",
                "media_formats": {
                    "gif": { "url": "https://static.klipy.com/ii/abc/de/fg/cat-typing-x1.gif", "dims": [498, 280] },
                    "tinygif": { "url": "https://static.klipy.com/ii/abc/de/fg/cat-typing-t2.gif", "dims": [220, 124] },
                    "preview": { "url": "https://static.klipy.com/ii/abc/de/fg/cat-typing-p3.png", "dims": [498, 280] },
                    "mp4": { "url": "https://static.klipy.com/ii/abc/de/fg/cat-typing-m4.mp4", "dims": [498, 280] },
                    "webm": { "url": "https://static.klipy.com/ii/abc/de/fg/cat-typing-w5.webm", "dims": [498, 280] }
                }
            },
            {
                "id": "no-dims",
                "media_formats": { "gif": { "url": "https://static.klipy.com/ii/x/x.gif", "dims": [] } }
            }
        ],
        "next": "30"
    }"#;

    #[test]
    fn parses_results_and_skips_gifs_without_dimensions() {
        let page = parse_page(KLIPY).unwrap();
        assert_eq!(page.next.as_deref(), Some("30"));
        assert_eq!(page.gifs.len(), 1);
        let g = &page.gifs[0];
        assert_eq!((g.width, g.height), (498, 280));
        assert_eq!(g.description, "Cat Typing GIF");
        assert!(g.tiny_url.ends_with("-t2.gif"));
        assert!(g.preview_url.ends_with(".png"));
        assert_eq!(g.mp4_slug.as_deref(), Some("cat-typing-m4"));
        assert_eq!(g.webm_slug.as_deref(), Some("cat-typing-w5"));
    }

    #[test]
    fn klipy_embed_uri_matches_the_bluesky_apps_and_round_trips() {
        let g = &parse_page(KLIPY).unwrap().gifs[0];
        let uri = g.embed_uri();
        assert_eq!(
            uri,
            "https://static.klipy.com/ii/abc/de/fg/cat-typing-x1.gif?hh=280&ww=498&mp4=cat-typing-m4&webm=cat-typing-w5"
        );
        assert_eq!(gif_embed_dims(&uri), Some((498, 280)));
    }

    #[test]
    fn tenor_gifs_already_in_timelines_still_play() {
        assert_eq!(
            gif_embed_dims("https://media.tenor.com/abcAAAAC/cat.gif?hh=280&ww=498"),
            Some((498, 280))
        );
        // Tenor-hosted GIFs never get video slugs appended.
        let g = Gif {
            id: "t".into(),
            url: "https://media.tenor.com/abcAAAAC/cat.gif".into(),
            width: 10,
            height: 20,
            tiny_url: String::new(),
            preview_url: String::new(),
            description: String::new(),
            mp4_slug: Some("x".into()),
            webm_slug: None,
        };
        assert_eq!(
            g.embed_uri(),
            "https://media.tenor.com/abcAAAAC/cat.gif?hh=20&ww=10"
        );
    }

    #[test]
    fn unknown_or_malformed_links_are_not_gifs() {
        assert_eq!(gif_embed_dims("https://example.com/a.gif?hh=1&ww=1"), None);
        // Missing size params — the official apps won't play it either.
        assert_eq!(gif_embed_dims("https://static.klipy.com/ii/a/b.gif"), None);
        // KLIPY outside /ii/, Tenor with a non-GIF format id.
        assert_eq!(
            gif_embed_dims("https://static.klipy.com/other/b.gif?hh=1&ww=1"),
            None
        );
        assert_eq!(
            gif_embed_dims("https://media.tenor.com/abcAAAAM/a.gif?hh=1&ww=1"),
            None
        );
    }

    #[test]
    fn description_prefixes_follow_the_official_apps() {
        assert_eq!(gif_description("Cat Typing GIF", ""), "ALT: Cat Typing GIF");
        assert_eq!(
            gif_description("Cat Typing GIF", "  my cat  "),
            "Alt: my cat"
        );
    }
}
