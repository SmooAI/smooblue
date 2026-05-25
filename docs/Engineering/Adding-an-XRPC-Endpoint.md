# Adding an XRPC Endpoint

#engineering

How to wire a new `app.bsky.*` or `com.atproto.*` endpoint into the client.

---

## 1. Decode the response

Find the lexicon definition (https://atproto.com/specs/lexicon or `lexicon.community`). Add the response type to `crates/smooblue-atproto/src/feed.rs` (or a sibling module if it's not feed-related).

Style notes:
- `#[serde(rename = "camelCase")]` per-field — the lexicon is camelCase, our Rust idioms are snake_case
- `#[serde(default)]` on every optional field. Lexicon fields drift; missing-field-equals-default is a softer landing than failing the whole decode
- For fields the AppView ships with variable shapes (`reply.parent` can be a real PostView, `notFoundPost`, or `blockedPost`), use `serde_json::Value` and write a small helper method instead of an `#[serde(untagged)]` enum. The latter often produces opaque "no variant matched" errors that are hard to debug; the former defaults to "ignore" and you control the failure mode.

---

## 2. Add the client method

In `crates/smooblue-atproto/src/client.rs`, add `pub async fn` returning `Result<YourResponse, AtError>`. Pattern:

```rust
pub async fn get_whatever(&self, foo: &str) -> Result<WhateverResponse, AtError> {
    let mut url = self
        .session_pds_url("/xrpc/app.bsky.foo.getWhatever")
        .map_err(|e| AtError::Decode(e.to_string()))?;
    url.query_pairs_mut().append_pair("foo", foo);
    self.get_json(&url).await
}
```

For POSTs to repo writes (`com.atproto.repo.createRecord`, `putRecord`, `deleteRecord`), build the body as a `serde_json::json!({...})` and use `self.post_json(&url, &body).await`.

`session_pds_url` routes through the user's PDS — `app.bsky.*` endpoints **must** go through the PDS so the PDS can sign service-auth for the AppView on the user's behalf. Hitting the AppView directly with a user token returns 401 AuthMissing.

---

## 3. Add a wiremock test

```rust
#[tokio::test]
async fn get_whatever_decodes_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/xrpc/app.bsky.foo.getWhatever"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "whatever": [{ "uri": "at://x", "value": 42 }]
        })))
        .mount(&server)
        .await;
    let client = AtClient::new(fake_session(&server.uri()), Url::parse(&server.uri()).unwrap());
    let resp = client.get_whatever("foo").await.unwrap();
    assert_eq!(resp.whatever[0].value, 42);
}
```

For POSTs, assert the request body shape with `respond_with(|req: &Request| {...})` — the lexicon image-key bug (`"blob"` vs `"image"`) was a one-line fix that would have been a 1.0-ship-blocker without an assertion test.

---

## 4. Wire into the UI

If it's a column-backing endpoint, see [[Adding-a-Column-Type]]. If it's a one-shot fetch (engagement modal, profile load, etc.), use `use_resource` and **read the focus/key signal inside the closure** — capturing it by value freezes the resource at first render. See [[../Architecture/Architecture-Overview#Reactive use_resource gotcha]].

---

## 5. Re-export from `lib.rs`

If callers in `smooblue-app` need the new response type or input type, add it to `crates/smooblue-atproto/src/lib.rs`'s `pub use` block. Missing re-exports is the most common "why won't this compile" papercut.

---

## Related

- [[Adding-a-Column-Type]]
- [[../Architecture/OAuth-and-Session]]
