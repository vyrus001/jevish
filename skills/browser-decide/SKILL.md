---
name: browser-decide
description: Resolve jevish choice questions with a local heuristic or any external model while emitting the same probability-bearing decision contract.
---

# Browser decide

Decision engines must return one probability distribution per question. Every key must come from that question's criteria.

For offline smoke tests:

```bash
jevish decide <questions.json>
```

The bundled heuristic is not a capable semantic model. Use it only when lexical matching is sufficient or to test plumbing. A generic harness may answer the JSON directly with any model, classifier, rules engine, or human selection.

Do not execute an action here. Do not replace `none` or `ambiguous` with a guess.

Pass decisions to `$browser-gate`.
