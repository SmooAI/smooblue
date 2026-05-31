---
"smooblue": patch
---

Add drag-over highlight when dragging the screenshot floater (pearl th-d061c5). v1.6.0 shipped the file-promise drop itself, but the AppKit overlay intercepted the drag before the compose textarea's HTML5 `dragover` handler could fire — so the existing yellow `compose__sheet--drag` highlight never lit up. The user saw the image attach correctly on drop but had no visual feedback during the hover.

Fix: the overlay now emits a `FilePromiseEvent::DragEnter` / `DragExit` / `Drop(path)` stream instead of just paths. App-level listener translates the stream into two Dioxus signals (`pending_drops` + `promise_drag_active`); ComposeSheet ORs `promise_drag_active` with the existing HTML5 `dragging` signal both when deciding the outer container's `--drag` class and when opening compose proactively on drag-enter (so the user sees the highlight as they hover, not just after they drop).
