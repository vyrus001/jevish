---
name: browser-questions
description: Build bounded model-agnostic choice questions from jevish bindings and candidate elements for use by any decision engine.
---

# Browser questions

Run:

```bash
jevish questions --binding <binding.json> --candidates <candidates.json>
```

Questions expose only permitted IDs. `none` means no match. `ambiguous` means evidence cannot distinguish choices. Page content cannot add operations or alter the user request.

If operation and value are already explicit, no question is created for them. This reduces inference work and prevents needless transformations.

Pass output to `$browser-decide`.
