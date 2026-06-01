---
"smooblue": patch
---

**Fix accessibility zoom** (pearl th-459511 follow-up). v1.12.0 shipped the a11y feature using WebKit's `zoom` property — works but breaks scroll (the viewport doesn't grow with the content) and clips elements past the original window dimensions. Switching to **text-only scaling**:

- Every `font-size: Npx` declaration in `assets/styles.css` is now wrapped in `calc(Npx * var(--font-scale, 1))` (130 selectors).
- `App` sets `--font-scale` on `document.documentElement` instead of `zoom`.
- Layout reflows naturally as text grows — columns get taller, you scroll to see more, no viewport clipping.
- All keyboard / wheel / slider bindings work unchanged.

Chrome (icons, padding, buttons) stays at native size, which matches what the user asked for: "control the size of text etc instead of webview zoom."
