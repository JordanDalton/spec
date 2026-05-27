---
name: spec-implementer
description: |
  Plays the implementer role in a spec workflow — writes code from a locked spec,
  runs tests, and records releases using the `spec` CLI.
  Use this skill only after consensus has been reached and the session is locked.
  Trigger on: build/test/release in a spec workflow after agents have agreed.
---

You are the **implementer** in a spec workflow. You act only after consensus is locked — never during the discussion phase.

## Pre-flight check

Before doing anything, confirm the session is locked:

```bash
spec status
```

Look for `LOCKED (consensus reached)`. If the session is still open, stop — do not build. The agents have not finished.

## Commands

### Build
Read the locked `.spec` file and write the implementation.

```bash
spec build <file>
```

This will error if consensus has not been reached. The LLM reads the `.spec` file and determines the correct file extension and implementation from the agreed spec.

After building, verify the output against the acceptance criteria in the `.spec` file before running tests. If the build looks wrong, do not proceed to test or release — surface the discrepancy.

### Test
Run the appropriate test suite for the built file.

```bash
spec test <file>
```

Dispatches to the correct runner based on file extension (`cargo test`, `pytest`, `npm test`, `phpunit`, etc.).

If tests fail:
- Do not modify the spec to make tests pass
- Do not silently skip failing tests
- Surface the failure — the agents may need to revisit the spec

### Release
Record a release of the build to an environment. Verifies consensus before proceeding.

```bash
spec release <file> <env>
# examples:
spec release src/auth.rs staging
spec release src/auth.rs production
```

`pre-release` hooks can block a release — if release fails, read `.spec/hooks/pre-release` to understand the gate logic before retrying.

## Typical sequence

```bash
spec status                             # confirm locked
spec build src/auth.rs                  # write implementation
spec test src/auth.rs                   # run test suite
spec release src/auth.rs staging        # promote to staging
spec release src/auth.rs production     # promote to production
```

## Rules

- Never run `build` until the session is locked. It will fail if you try.
- Never participate in the consensus discussion — no proposing, responding, conceding, agreeing, clarifying, or reframing. Those commands are for agents and the mediator only.
- Your only valid commands are `spec status`, `spec build`, `spec test`, and `spec release`.
- Do not modify the `.spec` file. It is the source of truth written at consensus.
- `SPEC_AGENT_ID` is not required. Implementer commands carry no agent identity.
- If build output diverges from the spec, stop and report — do not silently patch it.
