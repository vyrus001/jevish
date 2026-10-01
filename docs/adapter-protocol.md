# Browser adapter protocol

An adapter is an executable invoked once per request. It reads one JSON object from stdin, writes one JSON object to stdout, and sends diagnostics only to stderr. Exit nonzero only when no valid response can be produced.

All artifacts use contract `jevish.browser/v1`.

## Snapshot

Request:

```json
{"kind":"snapshot"}
```

Response:

```json
{"kind":"snapshot","data":{"contract":"jevish.browser/v1","backend":"example","snapshot_id":"...","document":{"url":"https://example.test","title":"Example","context_id":"tab-1","revision":"navigation-7"},"captured_at_ms":0,"nodes":[]}}
```

`context_id` must distinguish tabs or top-level browsing contexts. `revision` must change after document replacement. Raw nodes should use accessibility semantics rather than CSS selectors.

## Execute

Request:

```json
{"kind":"execute","plan":{"contract":"jevish.browser/v1","snapshot_id":"...","document":{},"operation":"click","target":{},"value":null,"confidence":0.94,"margin":0.51}}
```

Response uses `{"kind":"executed","data":...}`.

Before dispatch, adapters must compare current document identity with `plan.document`. Targeted operations must also verify target identity, visibility, enabled state, and semantic fingerprint. Return an error response instead of selecting a replacement target.

## Error

```json
{"kind":"error","data":{"code":"STALE_TARGET","message":"target identity changed"}}
```

Never place credentials, input secrets, or page dumps in error messages.
