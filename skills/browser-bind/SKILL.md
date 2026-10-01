---
name: browser-bind
description: Extract operation hints and exact user-provided values, URLs, keys, and scroll directions locally before browser decision inference.
---

# Browser bind

Run:

```bash
jevish bind '<user instruction>' [--operation <operation>]
```

Use `--operation` when the harness already knows it. Binding is deterministic and does not contact a model.

Preserve extracted strings exactly. Do not let a decision engine invent, rewrite, or normalize secret input. When a value is sensitive, prefer a harness-private value channel and place only its opaque reference in downstream artifacts.

Pass binding and candidates to `$browser-questions`.
