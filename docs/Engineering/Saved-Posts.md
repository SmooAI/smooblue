# Saved Posts (Bluesky bookmarks)

#engineering #feature

Save posts to read later, using Bluesky's own private bookmarks (`app.bsky.bookmark.*`). This is the same "Saved" list bsky.app and the Bluesky mobile apps show, so a post saved on your phone shows up in Smooblue's Saved column and vice versa. Bookmarks are private: they're stored by the AppView for your account, not as public records in your repo.

---

## User flow

- **Save / unsave:** the bookmark button in every post's action row, after Like. It fills (Smoo orange) when saved.
- **Saved column:** the bookmark button in the rail, **g b**, or a saved deck layout. Newest saves come first, with infinite scroll. Column filters (media-only etc.) and the text filter work as they do on any feed.
- An unsave, from any column, drops the post out of the Saved column immediately. A save made elsewhere appears within ~20 s (the column's poll).

---

## How it works

| Piece | Where |
| --- | --- |
| API | `AtClient::create_bookmark(uri, cid)`, `delete_bookmark(uri)`, `get_bookmarks(cursor, limit ≤ 100)`. All go through the user's PDS, which proxies to the AppView, the same path as `muteActor`. |
| Types | `PostViewerState::bookmarked`. `BookmarksResponse::into_feed()` turns `bookmarkView` items (a `postView` / `notFoundPost` / `blockedPost` union) into ordinary `FeedItem`s and drops deleted or blocked posts. |
| Column | `ColumnKind::Bookmarks` / `ColumnSpec::bookmarks()` ("Saved"). It's paginated, polls every 20 s, and renders through the standard PostCard path. |
| Toggle | `PostCard` saves optimistically (`OptimisticPostState::bookmarked`) and rolls back if the request fails. No record URI is tracked: a bookmark is addressed by the post URI. |
| Demo | `demo::saved_feed()`: a few home-feed posts marked saved. In demo mode toggles are local only. |
