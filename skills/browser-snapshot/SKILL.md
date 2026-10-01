---
name: browser-snapshot
description: Capture a versioned raw browser accessibility snapshot with the jevish CLI when browser state must be exposed without screenshots or model-specific tooling.
---

# Browser snapshot

Use `jevish snapshot` to obtain `jevish.browser/v1` raw accessibility data.

For CDP, obtain the exact page WebSocket endpoint from the browser controller, then run:

```bash
jevish snapshot --backend cdp --endpoint <page-websocket-url>
```

For another browser controller, use its adapter:

```bash
jevish snapshot --backend process --adapter <adapter-command> -- <adapter-arguments>
```

Do not infer actions from the raw tree. Preserve `snapshot_id`, `document.context_id`, and `document.revision`; later execution uses them for stale-page rejection.

Pass output to `$browser-elements`.
