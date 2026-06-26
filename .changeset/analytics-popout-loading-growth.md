---
"smooblue": minor
---

Analytics page improvements:

- **Pop out to a full page.** An expand button in the Analytics column header opens a wider, two-column version of the dashboard (Esc or the ✕ closes it).
- **Real loading states.** Each card now shows a spinner while its data is still being reconstructed in the background, instead of looking empty or showing misleading partial data. Driven by the backfill phase.
- **Growth chart now spans your full history.** It previously sampled only the last 30 days, which made a years-old account read as a flat line; it now plots the real follower/following curve from your first follow onward.
- **Top posts wait for engagement.** The list shows a loading state until like/repost counts are backfilled, so it ranks by actual engagement rather than falling back to most-recent.
- Analytics columns now re-read every 20s (was 5 min) so the charts visibly fill in as the backfill progresses.
