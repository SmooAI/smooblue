---
"smooblue": minor
---

Add an **Analytics** page (new rail button → Analytics column). It charts your account over time, reconstructed from the follow graph — no third-party site needed:

- **Followers vs following over time** and **posts over time** (inline-SVG growth curves), plus a **posting-cadence heatmap** (weekday × hour).
- **Best followers**, ranked by a clout score that blends their reach, how often they engage with you, whether you follow them back, and recency.
- **Best posts** by engagement.

Data is reconstructed once (your repo for following/posts, the Constellation backlink index for followers, with each follow's TID timestamp) and cached in the local SQLite store, then a daily snapshot tracks true net counts going forward. The charts read the pre-aggregated data off the UI thread, so the page stays responsive while the background task backfills.
