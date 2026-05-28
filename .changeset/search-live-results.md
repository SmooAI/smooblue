---
"smooblue": minor
---

Search is now live results, not a column-builder. Typing in the search sheet fires a debounced `searchActorsTypeahead` + `searchPosts` in parallel; results appear in two stacked sections (Users + Posts). Clicking a user row opens their profile sheet; clicking a post row opens the thread. Each user row also has a "+ column" button to pin them as an author-feed column. The "Add as search column" footer button is still there if you want to materialise the current query as a permanent column — the old behaviour is now opt-in rather than the only option.
