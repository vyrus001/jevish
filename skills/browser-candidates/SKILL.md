---
name: browser-candidates
description: Restrict jevish elements to targets valid for one browser operation before any model or heuristic chooses a target.
---

# Browser candidates

Choose one supported operation, then run:

```bash
jevish candidates <elements.json> --operation <click|fill|type|check|uncheck|select|get-text|focus|press|scroll|open>
```

Prefer an explicit operation supplied by the user or harness. Do not widen the candidate set to compensate for a missing target. An empty set means the current page and scope offer no valid target.

Run `$browser-bind` on the original user instruction, then pass both artifacts to `$browser-questions`.
