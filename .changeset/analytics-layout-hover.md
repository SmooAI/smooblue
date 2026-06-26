---
"smooblue": patch
---

Analytics pop-out layout + chart polish:

- **No more horizontal scrolling.** The deep-dive modal was capped at the base sheet width (560px) and its grid couldn't shrink, forcing ~440px of horizontal overflow; the modal is now properly wide and the columns shrink (follower names truncate) so everything fits. Verified against the running app.
- **Better organized.** The follower-lens cards now sit in the right column beside the charts (via dense grid flow) instead of being pushed to the bottom, leaving the right side empty.
- **Hover points on the charts.** Hovering a line chart reveals a marker at each data point with a tooltip ("2026-06 · Followers: 1,040"); bars and the posting-cadence heatmap cells show tooltips too.
