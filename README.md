# jevish

Model-agnostic browser automation for agent harnesses.

`jevish` turns a browser accessibility tree into small, versioned JSON decisions. Browser observation and execution stay separate from model inference, so a harness can use an LLM, classifier, rules engine, human review, or no model at all.

> Project status: early development. The JSON contract is versioned as `jevish.browser/v1`, but compatibility before `1.0` is not guaranteed. Chrome DevTools Protocol (CDP) is the first native backend. Other browsers can integrate through the process-adapter protocol.

## Why jevish

General browser agents often send screenshots or large DOMs to a model and ask it to generate an action. `jevish` narrows that problem:

- Rust captures and normalizes accessibility data.
- Operation-specific filters expose only actionable candidates.
- Deterministic parsing binds explicit operations and values locally.
- Models choose bounded IDs instead of generating selectors or commands.
- Confidence gates reject uncertain or malformed decisions.
- Execution revalidates document and target identity before dispatch.
- Every stage uses portable JSON and can be replaced independently.

## Decision pipeline

```text
accessibility tree
  │
  ├─ 1. snapshot      raw accessibility nodes + document identity
  ├─ 2. extract       compact semantic elements, context, fingerprints
  ├─ 3. candidates    targets valid for one operation
  ├─ 4. bind          exact local values, URLs, keys, directions
  ├─ 5. questions     bounded choices with none/ambiguous escapes
  ├─ 6. decide        replaceable decision engine
  ├─ 7. gate          probability validation + immutable action plan
  └─ 8. execute       freshness checks + browser action
```

The model, when present, never executes browser actions. Page content is untrusted data and cannot add operations, change the user request, or authorize side effects.

## Installation

Requirements for source installation:

- Rust 1.85 or newer
- Bash on macOS or Linux
- A compatible browser endpoint or process adapter for live automation

Install the CLI and shared skill bundle under `~/.local`:

```bash
./scripts/install.sh
```

Install skills directly into a supported harness:

```bash
./scripts/install.sh --harness codex
./scripts/install.sh --harness claude
./scripts/install.sh --harness agents
./scripts/install.sh --harness project
```

Any harness can supply its own skill directory:

```bash
./scripts/install.sh --skills-dir /path/to/harness/skills
```

Multiple destinations may be installed together:

```bash
./scripts/install.sh \
  --harness codex \
  --harness claude \
  --skills-dir /opt/custom-agent/skills
```

Other useful modes:

```bash
./scripts/install.sh --no-skills --prefix /opt/jevish
./scripts/install.sh --no-cli --skills-dir ./vendor/skills
./scripts/install.sh --binary ./jevish --skills-dir ./agent/skills
./scripts/install.sh --dry-run --harness project
./scripts/install.sh --prefix ~/.local --uninstall
```

The installer does not overwrite an unmanaged skill directory unless `--force` is supplied. Forced conflicts move to timestamped backups. Uninstall removes only paths recorded in the installation manifest.

## Quick start

Start Chrome or Chromium with a dedicated debugging profile. Do not expose its debugging port to an untrusted network.

```bash
chromium \
  --remote-debugging-port=9222 \
  --user-data-dir=/tmp/jevish-browser-profile
```

Find the target page's `webSocketDebuggerUrl` from `http://127.0.0.1:9222/json/list`, then capture its accessibility tree:

```bash
jevish snapshot \
  --backend cdp \
  --endpoint ws://127.0.0.1:9222/devtools/page/PAGE_ID \
  > raw.json
```

Run each portable stage:

```bash
jevish extract raw.json > elements.json

jevish candidates elements.json \
  --operation click \
  > candidates.json

jevish bind 'click the "Continue" button in Checkout' \
  --operation click \
  > binding.json

jevish questions \
  --binding binding.json \
  --candidates candidates.json \
  > questions.json

jevish decide questions.json > decisions.json

jevish gate \
  --questions questions.json \
  --decisions decisions.json \
  --candidates candidates.json \
  --binding binding.json \
  > plan.json

jevish execute plan.json \
  --backend cdp \
  --endpoint ws://127.0.0.1:9222/devtools/page/PAGE_ID
```

`jevish decide` is an offline lexical heuristic intended for smoke tests and straightforward matching. Production harnesses should answer `QuestionBundle` with their chosen decision system and emit the same `DecisionBundle` contract.

## Decision-engine integration

A question contains instructions and a closed criterion set:

```json
{
  "id": "target",
  "instruction": "Choose the one actionable element matching the user request.",
  "criteria": [
    {"id": "e1", "label": "button \"Continue\" in region: Checkout"},
    {"id": "e2", "label": "button \"Cancel\" in region: Checkout"},
    {"id": "none", "label": "No matching element"},
    {"id": "ambiguous", "label": "Several elements match equally"}
  ]
}
```

A decision engine returns one normalized probability distribution:

```json
{
  "question_id": "target",
  "choice": "e1",
  "probabilities": {
    "e1": 0.96,
    "e2": 0.02,
    "none": 0.01,
    "ambiguous": 0.01
  }
}
```

The gate rejects:

- IDs absent from the original criteria;
- empty distributions, unknown IDs, or non-normalized distributions (zero-probability choices may be omitted);
- `none` and `ambiguous` outcomes;
- selected probability below the configured threshold;
- winning margin below the configured threshold;
- mismatched snapshot IDs or operations.

## Browser backends

### CDP

The built-in CDP backend captures `Accessibility.getFullAXTree`, resolves native backend node IDs, and implements navigation, click, focus, fill, type, check, uncheck, select, text retrieval, key presses, and scrolling.

Navigation-capable `open` and `click` actions use bounded CDP waits. When dispatch is established but navigation replaces the execution context before CDP returns the command response, execution returns `navigation_indeterminate` instead of hanging. An observed completed navigation returns `navigation_completed` with structured navigation metadata. Harnesses must take a fresh snapshot before the next action after either result; document and target freshness checks still apply to every new plan.

Navigation timing is separate from ordinary CDP command timing. Ordinary commands retain a 30-second response allowance, while full accessibility snapshots allow 120 seconds for large, complex pages.

Pass a page-level `ws://` or `wss://` endpoint. Browser-level CDP endpoints are not currently resolved into page sessions automatically.

### Process adapters

Any controller can implement the browser-neutral request/response protocol:

```bash
jevish snapshot \
  --backend process \
  --adapter ./my-browser-adapter \
  -- --profile test

jevish execute plan.json \
  --backend process \
  --adapter ./my-browser-adapter \
  -- --profile test
```

Adapters read one JSON request from stdin, write one JSON response to stdout, and send diagnostics to stderr. See [the adapter protocol](docs/adapter-protocol.md) for the wire format and required freshness checks.

## Safety model

An `ActionPlan` binds:

- snapshot ID;
- document URL;
- browsing-context ID;
- document-loader revision;
- exact operation and bound value;
- native target identity;
- semantic target fingerprint.

Before executing a targeted action, the CDP backend recaptures accessibility state and verifies document identity, target presence, fingerprint, visibility, and enabled state. Changed state produces `STALE_DOCUMENT` or `STALE_TARGET`; `jevish` never silently retargets.

Keep credentials and sensitive values out of decision artifacts. Use a harness-private input channel or browser adapter when secrets must be entered. Browser debugging endpoints have broad control over their profiles and must remain local and isolated.

## Skill bundle

The installer ships eight composable skills:

- `browser-automation` — primary router for live browser work
- `browser-snapshot` — accessibility capture
- `browser-elements` — Rust element extraction
- `browser-candidates` — operation-specific filtering
- `browser-bind` — deterministic instruction binding
- `browser-questions` — bounded question compilation
- `browser-decide` — replaceable decision resolution
- `browser-gate` — confidence checks, freshness, and execution

[Harness metadata](dist/harness-manifest.json) exposes the same pipeline for systems that do not consume `SKILL.md` directly.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
bash -n scripts/install.sh tests/install.sh
./tests/install.sh target/debug/jevish
```

Live CDP testing requires a dedicated browser profile. Unit and installer tests do not touch a user's browser or global skill directories.

Repository layout:

```text
src/                    Rust CLI, contracts, pipeline, and backends
skills/                 portable agent skills
docs/adapter-protocol.md
dist/harness-manifest.json
scripts/install.sh
tests/
```

See [CONTRIBUTING.md](CONTRIBUTING.md) before submitting changes and [SECURITY.md](SECURITY.md) for vulnerability reporting and browser-endpoint precautions.

## License

Apache License 2.0. See [LICENSE](LICENSE).
