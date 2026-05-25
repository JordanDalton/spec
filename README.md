# Spec

> **Prototype.** The core ideas are solid but the implementation is early — there will be rough edges, missing cases, and better ways to do things we haven't thought of yet. If something feels wrong or could be better, open an issue. Opinions, feedback, and contributions are welcome.

Version control and coordination system for AI agents. Every code file has a paired `.spec` file. Agents collaborate on the spec — reaching consensus — before any code is written. The code is always the *output* of consensus, never the battleground.

## Spec and Git

Spec is not a git replacement — it complements git. They solve different problems:

- **Git** tracks code changes over time — what changed, when, and by whom
- **Spec** coordinates intent before code is written — what should be built and why

### What to commit

Commit your `.spec` files alongside your code — they're the source of truth for intent and should live in version control with the implementation. The session logs in `.spec/sessions/` are worth committing too — they're the full audit trail of how consensus was reached.

```bash
git add src/auth.php src/auth.php.spec .spec/sessions/
git commit -m "add JWT validation to auth controller"
```

Your `.gitignore` should **not** exclude `*.spec` or `.spec/sessions/`.

### Branches

`.spec` files and sessions are branch-scoped — they switch when you checkout. This is correct. A feature branch has its own spec files and sessions that represent the intent for that branch.

### The lesson graph is global

Lessons live at `~/.spec/lessons.json` — outside the repo entirely. They accumulate across all projects and branches, and persist regardless of what you checkout. This is intentional: lessons are knowledge about *how your agents work*, not knowledge about any particular codebase.

### A typical workflow

```bash
# Agents reach consensus, implementer writes the code
spec-alice propose src/auth.php "add JWT validation"
spec-bob respond src/auth.php
spec-alice agree src/auth.php
spec-bob agree src/auth.php
spec build src/auth.php

# Commit spec + implementation together
git add src/auth.php src/auth.php.spec .spec/sessions/
git commit -m "add JWT validation to auth controller"
```

## Install

```bash
cargo install --path .
```

Requires `curl` on your PATH (used for LLM API calls).

## Setup

```bash
export SPEC_PROVIDER=anthropic
export SPEC_API_KEY=your-key-here
```

## Quick Start

```bash
# 1. Initialize a project
cd your-project
spec init

# 2. Agent A proposes
export SPEC_AGENT_ID=alice
spec propose app/Http/Controllers/HomeController.php "basic controller with invoke method returning 'testing'"

# 3. Agent B responds
export SPEC_AGENT_ID=bob
spec respond app/Http/Controllers/HomeController.php

# 4. If stuck, mediator clarifies
spec clarify app/Http/Controllers/HomeController.php

# 5. Agents concede or revise
export SPEC_AGENT_ID=alice
spec concede app/Http/Controllers/HomeController.php

# 6. Both agents sign off (consensus requires at least 2 distinct agents)
export SPEC_AGENT_ID=alice
spec agree app/Http/Controllers/HomeController.php

export SPEC_AGENT_ID=bob
spec agree app/Http/Controllers/HomeController.php
# → "CONSENSUS REACHED — Session locked"

# 7. Implementer writes the code
spec build app/Http/Controllers/HomeController.php

# 8. Run tests
spec test app/Http/Controllers/HomeController.php

# 9. Release
spec release app/Http/Controllers/HomeController.php production
```

## How It Works

### File Pairing

You always pass the **source file**. Spec automatically appends `.spec`:

```
app/Http/Controllers/HomeController.php       ← you reference this
app/Http/Controllers/HomeController.php.spec  ← Spec manages this
```

The `.spec` file is plain human-readable markdown — the agreed description of what the code file is supposed to do. It is written by Spec when consensus is reached, not by hand.

### Project Structure

`spec init` creates a `.spec/` folder in your project root (like `.git`):

```
your-project/
  .spec/
    config.json          — LLM provider and model configuration
    sessions/            — session logs per spec file (JSON)
    lessons/
      lessons.json       — lesson graph (learned from past failures)
  app/Http/Controllers/
    HomeController.php                — the code (produced by spec build)
    HomeController.php.spec           — the spec (written at consensus)
```

### The Flow

```
Agent A proposes → Agent B responds → Mediator clarifies (if stuck)
  → Agents concede/revise → All agents agree → Session locks
  → Implementer writes the code
```

- A build is **never** produced until all agents have explicitly agreed
- Consensus requires **at least 2 distinct agents** — one agent cannot lock a session alone
- The `.spec` file is only written when the session fully locks

## Agent Identity

**`SPEC_AGENT_ID` is required** for all agent commands (`propose`, `respond`, `concede`, `agree`). Without it, Spec errors with a clear message.

```bash
export SPEC_AGENT_ID=alice
spec propose app/Http/Controllers/HomeController.php "..."
```

This is intentional. Agent identity must be stable and explicit across commands — if each invocation got a different ID (e.g. `hostname:pid`), the system would treat them as different agents and consensus would never be reachable.

Set it once per terminal session with `export`, or prefix each command:

```bash
SPEC_AGENT_ID=alice spec propose app/Http/Controllers/HomeController.php "..."
SPEC_AGENT_ID=alice spec agree app/Http/Controllers/HomeController.php
```

## Commands

### `spec init`

Initialize a spec project in the current directory. Creates `.spec/` with config and empty session/lesson stores.

### `spec propose <file> "<intent>"`

An agent reads the current spec, queries the lesson graph, and proposes a change with full reasoning. Pass the source file — Spec manages the `.spec` file automatically.

```bash
SPEC_AGENT_ID=alice spec propose app/Http/Controllers/AuthController.php "add JWT validation with 1-hour expiry"
```

### `spec respond <file>`

A second agent reads the current session, reviews the latest proposal, and responds — accepting, rejecting, or suggesting modifications.

```bash
SPEC_AGENT_ID=bob spec respond app/Http/Controllers/AuthController.php
```

### `spec concede <file>`

An agent reviews the discussion and updates or withdraws its position.

```bash
SPEC_AGENT_ID=alice spec concede app/Http/Controllers/AuthController.php
```

### `spec agree <file>`

An agent explicitly signs off on the current spec state. When **all agents** (minimum 2) have agreed, the session locks, the `.spec` file is written, and the build becomes available.

```bash
SPEC_AGENT_ID=alice spec agree app/Http/Controllers/AuthController.php
SPEC_AGENT_ID=bob   spec agree app/Http/Controllers/AuthController.php
# → "CONSENSUS REACHED — Session locked"
```

### `spec clarify <file>`

Mediator-only. Surfaces a semantic contradiction without taking a position or voting.

```bash
spec clarify app/Http/Controllers/AuthController.php
```

### `spec reframe <file>`

Mediator-only. Reframes a stuck disagreement to help agents find common ground.

```bash
spec reframe app/Http/Controllers/AuthController.php
```

### `spec build <file>`

The implementer reads the locked spec and writes the implementation. Errors if consensus has not been reached. The LLM determines the correct file extension from context.

```bash
spec build app/Http/Controllers/HomeController.php
# → writes HomeController.php from the agreed spec
```

### `spec test <file>`

Runs tests for the built file. Dispatches to the appropriate test runner based on file extension (`cargo test`, `pytest`, `npm test`, `phpunit`, etc.).

```bash
spec test app/Http/Controllers/HomeController.php
```

### `spec release <file> <env>`

Records a release of the build to an environment. Verifies consensus before allowing release.

```bash
spec release app/Http/Controllers/HomeController.php production
spec release app/Http/Controllers/HomeController.php staging
```

### `spec status`

Shows the current state of the project — all spec files, open and locked sessions, agents involved, consensus state, lesson count.

```bash
spec status
```

### `spec log <file>`

Pretty-prints the full session history — agent IDs, message types, timestamps, proposals, and reasoning.

```bash
spec log app/Http/Controllers/HomeController.php
```

### `spec lessons`

Lists all lessons in the lesson graph with their provenance.

```bash
spec lessons
```

## Environment Variables

| Variable | Required | Description |
|---|---|---|
| `SPEC_AGENT_ID` | **Yes** (for agent commands) | Stable agent identity. Must be set before `propose`, `respond`, `concede`, `agree`. |
| `SPEC_PROVIDER` | Yes | LLM provider: `anthropic`, `openai`, `ollama`. Overrides `.spec/config.json`. |
| `SPEC_API_KEY` | Yes | API key for the selected provider. Isolated from your app's own API keys. |
| `SPEC_MODEL` | No | Model override (e.g. `claude-opus-4-7`, `gpt-4o`). Overrides `.spec/config.json`. |

### Key isolation

`SPEC_PROVIDER` and `SPEC_API_KEY` are deliberately separate from whatever keys your app uses. If your project already exports `ANTHROPIC_API_KEY` for its own calls, Spec won't touch it:

```bash
# Your app's key — Spec ignores this
export ANTHROPIC_API_KEY=sk-prod-app-key

# Spec's own keys — completely isolated
export SPEC_PROVIDER=anthropic
export SPEC_API_KEY=sk-spec-billing-key
export SPEC_MODEL=claude-sonnet-4-6   # optional
```

Fallback order for API key resolution:
1. `SPEC_API_KEY`
2. Provider-specific key (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`)
3. `anthropic_api_key_env` field in `.spec/config.json`

## Configuration

`.spec/config.json` is created by `spec init` and controls defaults:

```json
{
  "llm_provider": "anthropic",
  "model": "claude-sonnet-4-6",
  "anthropic_api_key_env": "ANTHROPIC_API_KEY"
}
```

Environment variables always take precedence over this file.

**Supported providers:**

| Provider | `llm_provider` value | Key env var |
|---|---|---|
| Anthropic | `anthropic` | `SPEC_API_KEY` or `ANTHROPIC_API_KEY` |
| OpenAI | `openai` | `SPEC_API_KEY` or `OPENAI_API_KEY` |
| Ollama (local) | `ollama` | — (set `OLLAMA_HOST` or defaults to `localhost:11434`) |

## Defining Agents

The cleanest way to work with multiple agents is to define a shell alias for each one in your `~/.zshrc` or `~/.bashrc`:

```bash
alias spec-alice="SPEC_AGENT_ID=alice spec"
alias spec-bob="SPEC_AGENT_ID=bob spec"
alias spec-carol="SPEC_AGENT_ID=carol spec"
```

Then use them directly — no `export` or inline env var needed:

```bash
spec-alice propose app/Http/Controllers/HomeController.php "basic controller returning 'testing'"
spec-bob respond app/Http/Controllers/HomeController.php
spec-alice agree app/Http/Controllers/HomeController.php
spec-bob agree app/Http/Controllers/HomeController.php
spec build app/Http/Controllers/HomeController.php
```

Commands that don't involve an agent identity (`build`, `clarify`, `reframe`, `status`, `log`, `lessons`) are called as plain `spec` with no alias needed.

## Running Multiple Agents

Open multiple terminals, one per agent:

**Tab 1 — Alice**
```bash
export SPEC_AGENT_ID=alice
spec propose app/Http/Controllers/PaymentController.php "add idempotency keys to all mutations"
# ... after bob responds ...
spec agree app/Http/Controllers/PaymentController.php
```

**Tab 2 — Bob**
```bash
export SPEC_AGENT_ID=bob
spec respond app/Http/Controllers/PaymentController.php
spec agree app/Http/Controllers/PaymentController.php
```

**Tab 3 — Mediator (no SPEC_AGENT_ID needed)**
```bash
spec clarify app/Http/Controllers/PaymentController.php
spec reframe app/Http/Controllers/PaymentController.php
```

**Tab 4 — Implementer (runs last)**
```bash
spec build app/Http/Controllers/PaymentController.php
```

## Hooks

Spec calls executable scripts from `.spec/hooks/` at defined points in the lifecycle. This is how you plug in your own deployment pipelines, notifications, linters, or gate checks — without Spec needing to know anything about your infrastructure.

`spec init` creates example scripts for every hook (`.example` extension). To activate one:

```bash
cp .spec/hooks/pre-release.example .spec/hooks/pre-release
chmod +x .spec/hooks/pre-release
```

### Available Hooks

| Hook | When | Aborts on failure? |
|---|---|---|
| `post-agree` | Session locks — consensus reached | No |
| `post-build` | Code file written by implementer | No |
| `pre-release` | Before release is recorded | **Yes** |
| `post-release` | After release is recorded | No |

### Context

Every hook receives context via environment variables:

| Variable | Available in |
|---|---|
| `SPEC_FILE` | All hooks |
| `SPEC_SESSION_ID` | All hooks |
| `SPEC_ENV` | `pre-release`, `post-release` |

### Examples

**Notify Slack when consensus is reached (`post-agree`)**
```bash
#!/bin/sh
curl -s -X POST "$SLACK_WEBHOOK" \
  -H "Content-Type: application/json" \
  -d "{\"text\": \"Consensus reached on $SPEC_FILE\"}"
```

**Block production releases on Fridays (`pre-release`)**
```bash
#!/bin/sh
if [ "$SPEC_ENV" = "production" ] && [ "$(date +%u)" -eq 5 ]; then
  echo "No production releases on Fridays." >&2
  exit 1
fi
```

**Trigger a deployment pipeline (`post-release`)**
```bash
#!/bin/sh
curl -s -X POST "https://ci.example.com/deploy" \
  -H "Authorization: Bearer $CI_TOKEN" \
  -d "env=$SPEC_ENV&file=$SPEC_FILE"
```

**Run a linter after build (`post-build`)**
```bash
#!/bin/sh
./vendor/bin/phpstan analyse "${SPEC_FILE%.spec}"
```

Missing hooks are silently skipped — if `.spec/hooks/post-agree` doesn't exist, nothing happens.

## Key Invariants

- A build is never produced until all agents run `spec agree`
- Consensus requires at least 2 distinct agents — one agent cannot lock a session alone
- `SPEC_AGENT_ID` must be set for all agent commands — no implicit identity
- The `.spec` file is plain markdown, written only at consensus
- The mediator never votes, never decides, never proposes
- The implementer never participates in resolution
- Every message carries reasoning — proposals alone are not enough

## Built With

Rust. Three dependencies: `serde`, `serde_json`, `tokio`. LLM calls go over raw HTTP via `curl` — no SDKs.
