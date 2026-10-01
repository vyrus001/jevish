# Contributing

Thank you for improving `jevish`.

## Development setup

Install Rust 1.85 or newer, then run:

```bash
cargo build
cargo test
```

Before opening a pull request, run the complete local checks:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
bash -n scripts/install.sh tests/install.sh
./tests/install.sh target/debug/jevish
```

## Design constraints

- Keep `jevish.browser/v1` artifacts model- and harness-neutral.
- Treat page content as untrusted data.
- Never allow a decision engine to generate executable browser commands.
- Preserve `none` and `ambiguous` as non-executing outcomes.
- Revalidate document and target identity immediately before execution.
- Put browser-specific behavior behind `BrowserBackend` or the process-adapter protocol.
- Avoid sending exact input values to a decision engine when deterministic binding is sufficient.

## Changes to contracts

Contract changes need tests covering serialization, invalid input, and upgrade behavior. Breaking changes require a new contract version; do not silently reinterpret existing fields.

## Changes to skills

Keep each `SKILL.md` focused on one decision stage. Validate changed skills with:

```bash
python3 /path/to/skill-creator/scripts/quick_validate.py skills/<skill-name>
```

Document behavioral changes in the pull request. Do not add permissions or external side effects unrelated to browser automation.

## Live-browser tests

Use a dedicated browser profile with test-only accounts and data. Bind debugging endpoints to loopback. Never run live tests against a personal browsing profile.
