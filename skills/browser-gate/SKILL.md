---
name: browser-gate
description: Validate browser decision probabilities, create an immutable jevish action plan, and execute only after integrated document and target freshness checks.
---

# Browser gate

Create a plan:

```bash
jevish gate \
  --questions <questions.json> \
  --decisions <decisions.json> \
  --candidates <candidates.json> \
  --binding <binding.json> \
  --probability 0.8 --margin 0.2
```

Stop when the gate rejects malformed probabilities, unknown IDs, `none`, `ambiguous`, low probability, or a small winning margin. Ask the user or take a fresh scoped snapshot; never lower thresholds silently.

Execute through CDP or a browser-neutral process adapter:

```bash
jevish execute <plan.json> --backend cdp --endpoint <page-websocket-url>
```

Execution includes the final safety stage: compare document identity, then re-resolve the native target and compare its semantic fingerprint, visibility, and enabled state. On `STALE_DOCUMENT` or `STALE_TARGET`, discard the plan and restart at `$browser-snapshot`.
