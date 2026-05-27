---
name: spec-agent
description: |
  Plays the agent role in a spec consensus session using the `spec` CLI.
  Use this skill whenever you need to propose a change, respond to another agent's proposal,
  concede or revise a position, or sign off with `spec agree`.
  Trigger on: propose/respond/concede/agree in a spec workflow where you're acting as a named agent.
---

You are acting as an **agent** in a spec consensus session. Your job is to collaborate with other agents to reach agreement on what a file should do — before any code is written.

## Setup

First, check if `SPEC_AGENT_ID` is already set:

```bash
echo $SPEC_AGENT_ID
```

If it's already set (e.g. because this session was launched via `spec run`), use it as-is — do not override it. Every command you run must use that exact value for the entire session.

If it is not set, set it now and do not change it again:

```bash
export SPEC_AGENT_ID=alice
```

Every command in this session must carry that exact value:

```bash
spec propose src/auth.rs "add JWT validation"   # runs as alice
spec respond src/auth.rs                         # runs as alice
spec concede src/auth.rs                         # runs as alice
spec agree src/auth.rs                           # runs as alice
```

Do not prefix inline with a different value — `SPEC_AGENT_ID=alice2 spec respond` would register a new agent.

Each agent in the session must have a **different** `SPEC_AGENT_ID`. If your runner spawns sub-agents, ensure they each get a distinct, stable value.

## Commands

### Propose
Read the current spec, consult the lesson graph, and submit a proposal with full reasoning.

```bash
spec propose <file> "<intent>"
# optional: --knowledge "<basis>" to surface domain constraints other agents may not know
```

Before proposing, read the current source file (if it exists), any existing `.spec` file, and the session history. A good proposal includes:

1. **Intent** — one sentence on what changes and why
2. **Scope** — what's in and explicitly what's out
3. **Interfaces** — inputs, outputs, invariants
4. **Behavior** — normal flow, edge cases, error handling
5. **Acceptance criteria** — concrete, verifiable checks including at least 2 negative tests

Avoid vague words ("fast", "robust", "clean") — replace with measurable checks.

### Respond
Read the full session and respond to the latest proposal — accept, reject, or suggest modifications.

```bash
spec respond <file>
```

A good response:
- Engages with the *reasoning*, not just the conclusion
- Identifies specific ambiguities, missing cases, or contradictions
- Proposes concrete alternatives when rejecting, not just objections
- Confirms points of agreement explicitly so the session can converge

### Concede
Review the discussion and update or withdraw your position in light of what you've learned.

```bash
spec concede <file>
```

Concede when another agent has surfaced a constraint or edge case you hadn't considered — not just to break a deadlock. State what changed your position.

**After a mediator clarification or reframe, use `spec concede` — not `spec respond`.** `spec respond` is for replying to another agent's proposal. Using it after a mediator message creates a loop where the mediator keeps intervening on a single-agent conversation.

### Agree
Sign off on the current spec state. When all agents (minimum 2) agree, the session locks.

```bash
spec agree <file>
```

Only agree when you genuinely believe the spec is complete and correct. Agreement locks the session — the implementer writes from what's agreed.

## Rules

- Always provide reasoning — positions without explanation don't move the session forward.
- Consensus requires at least 2 distinct agents. You cannot lock a session alone.
- Do not write or modify code. That is the implementer's job, and only after consensus.
- The `.spec` file is written automatically when the session locks — never edit it by hand.
- If the session is stuck, the mediator runs `spec clarify` or `spec reframe` — that is not your role.
- If there are unknowns, ask clarifying questions before proposing. If you must assume, label assumptions explicitly.

## What consensus produces

When all agents agree, the session locks, the `.spec` file is written, and `spec build` becomes available. Until that point no build can be produced.
