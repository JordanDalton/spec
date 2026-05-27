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
# Orchestrator loads work into the queue
spec queue add src/auth.php "add JWT validation with 1-hour expiry"

# Agents pull from the queue and run consensus
SPEC_AGENT_ID=alice spec next
SPEC_AGENT_ID=alice spec propose src/auth.php "add JWT validation with 1-hour expiry"
SPEC_AGENT_ID=bob   spec respond src/auth.php
SPEC_AGENT_ID=alice spec agree src/auth.php
SPEC_AGENT_ID=bob   spec agree src/auth.php

# Implementer builds
spec build src/auth.php

# Commit spec + implementation together
git add src/auth.php src/auth.php.spec .spec/sessions/
git commit -m "add JWT validation to auth controller"
```

## Install

```bash
cargo install --path .
```

This installs the `spec` CLI. To also install the MCP server (for AI agent coordination via MCP):

```bash
cargo install --path spec-mcp
```

Both binaries should be on your `PATH`.

## Agent Skills

`spec init` automatically installs skills for Codex CLI and Claude Code when it detects them (`~/.codex` or `~/.claude`). To install or reinstall manually:

```bash
spec install-skills                   # auto-detect Codex or Claude Code
spec install-skills --target <dir>    # install to a specific directory
```

Skills live in `skills/` in this repo and are embedded in the binary — no network access needed.

## Setup

### Subscription-based (no API key)

If you use Claude Code or Codex CLI, Spec can piggyback on your existing subscription:

```bash
export SPEC_PROVIDER=claudecode   # uses the local `claude` CLI
# or
export SPEC_PROVIDER=codex        # uses the local `codex` CLI
```

No `SPEC_API_KEY` needed.

### Direct API access

```bash
export SPEC_PROVIDER=anthropic
export SPEC_API_KEY=your-key-here
```

## Walkthrough

A complete run from empty project to built file. Copy-paste these commands to verify the system works. Uses `SPEC_PROVIDER=claudecode` (requires Claude Code installed).

```bash
# 1. Create a test project
mkdir spec-test && cd spec-test
export SPEC_PROVIDER=claudecode
spec init

# 2. Load a task into the queue
spec queue add app/Http/Controllers/HomeController.php \
  "create a basic Laravel controller with __invoke(): string returning 'hello'"

spec queue list
# → [ ] task_abc123   app/Http/Controllers/HomeController.php   PENDING

# 3. Agent alice claims the task and proposes
SPEC_AGENT_ID=alice spec next
# → Task: task_abc123
# → Run: SPEC_AGENT_ID=alice spec propose app/Http/Controllers/HomeController.php "..."

SPEC_AGENT_ID=alice spec propose app/Http/Controllers/HomeController.php \
  "create a basic Laravel controller with __invoke(): string returning 'hello'"
# → shows proposal, prompts y/N (type y)
# → STATUS: WAITING_FOR_REPLY

# 4. Agent bob responds
spec state app/Http/Controllers/HomeController.php
# → STATUS: WAITING_FOR_REPLY

SPEC_AGENT_ID=bob spec respond app/Http/Controllers/HomeController.php
# → STATUS: WAITING_FOR_AGREE

# 5. Both agents agree — session locks
SPEC_AGENT_ID=alice spec agree app/Http/Controllers/HomeController.php
SPEC_AGENT_ID=bob   spec agree app/Http/Controllers/HomeController.php
# → CONSENSUS REACHED — Session locked

spec state app/Http/Controllers/HomeController.php
# → STATUS: LOCKED

# 6. Build the implementation
spec build app/Http/Controllers/HomeController.php
# → writes app/Http/Controllers/HomeController.php

# 7. Verify the queue auto-completed the task
spec queue list
# → [✓] task_abc123   app/Http/Controllers/HomeController.php   DONE

# 8. Check the full session history
spec log app/Http/Controllers/HomeController.php
```

**If agents disagree** and the session goes STUCK, the mediator intervenes:

```bash
spec state app/Http/Controllers/HomeController.php
# → STATUS: STUCK

spec clarify app/Http/Controllers/HomeController.php
# → surfaces the contradiction; agents must now respond/concede, not re-propose

SPEC_AGENT_ID=alice spec respond app/Http/Controllers/HomeController.php
SPEC_AGENT_ID=bob   spec concede app/Http/Controllers/HomeController.php
SPEC_AGENT_ID=alice spec agree   app/Http/Controllers/HomeController.php
SPEC_AGENT_ID=bob   spec agree   app/Http/Controllers/HomeController.php
spec build app/Http/Controllers/HomeController.php
```

**If a session gets contaminated** and you need a clean start:

```bash
spec reset app/Http/Controllers/HomeController.php
# → session cleared; propose again from scratch
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
    queue.json           — task queue (added by orchestrator, claimed by agents)
    sessions/            — session logs per spec file (JSON)
    lessons/
      lessons.json       — lesson graph (learned from past failures)
  app/Http/Controllers/
    HomeController.php                — the code (produced by spec build)
    HomeController.php.spec           — the spec (written at consensus)
```

### The Flow

```
Orchestrator loads queue → Agents claim tasks (spec next) → Agent A proposes
  → Agent B responds → Mediator clarifies/reframes (if stuck)
  → Agents concede/revise → All agents agree → Session locks
  → Implementer writes the code
```

- A build is **never** produced until all agents have explicitly agreed
- Consensus requires **at least 2 distinct agents** — one agent cannot lock a session alone (use `--solo` to bypass for single-agent workflows)
- The `.spec` file is only written when the session fully locks
- After a mediator runs `clarify` or `reframe`, agents must use `respond` or `concede` — new proposals are blocked until consensus is reached

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

Initialize a spec project in the current directory. Creates `.spec/` with config and empty session/lesson stores. Automatically installs agent skills to `~/.codex/skills/` and `~/.claude/skills/` if those tools are detected.

### `spec propose <file> "<intent>" [--knowledge "<basis>"]`

An agent reads the current spec, queries the lesson graph, and proposes a change with full reasoning. Pass the source file — Spec manages the `.spec` file automatically.

```bash
SPEC_AGENT_ID=alice spec propose app/Http/Controllers/AuthController.php "add JWT validation with 1-hour expiry"

# With explicit knowledge basis
SPEC_AGENT_ID=alice spec propose app/Http/Controllers/AuthController.php "add JWT validation" \
  --knowledge "We use the firebase/php-jwt library. Tokens must expire in 3600s."
```

When running interactively (human at a terminal), the generated proposal is printed and requires confirmation (`y/N`) before it's committed to the session. Automated agents pipe stdin and skip the prompt automatically.

Output ends with `STATUS: WAITING_FOR_REPLY` — a machine-readable line agents can check without burning LLM tokens. Poll with `spec state <file>`.

**Blocked after mediation:** if a mediator has run `clarify` or `reframe` on this session, `propose` is blocked. Agents must use `respond` or `concede` instead.

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

### `spec agree <file> [--solo]`

An agent explicitly signs off on the current spec state. When **all agents** (minimum 2) have agreed, the session locks, the `.spec` file is written, and the build becomes available.

```bash
SPEC_AGENT_ID=alice spec agree app/Http/Controllers/AuthController.php
SPEC_AGENT_ID=bob   spec agree app/Http/Controllers/AuthController.php
# → "CONSENSUS REACHED — Session locked"
```

Use `--solo` to lock immediately without requiring a second agent:

```bash
SPEC_AGENT_ID=alice spec agree app/Http/Controllers/AuthController.php --solo
# → "SOLO AGREEMENT — Session locked"
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

### `spec state <file>`

Returns a single machine-readable status line — no LLM call, no noise. Designed for agents to poll cheaply.

```
STATUS: WAITING_FOR_REPLY    — proposal submitted, no response yet
STATUS: WAITING_FOR_AGREE    — discussion active, agents need to sign off
STATUS: STUCK                — competing proposals from 2+ agents, or ≥3 rounds with no agreement
STATUS: LOCKED               — consensus reached, build available
STATUS: NO_SESSION           — no session exists for this file
```

```bash
spec state app/Http/Controllers/HomeController.php
# → STATUS: WAITING_FOR_REPLY
```

Agents can use this in a loop without burning tokens:

```bash
until spec state <file> | grep -q "WAITING_FOR_AGREE\|LOCKED"; do
  sleep 10
done
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

### `spec reset <file>`

Clears the session for a spec file — removes the active session and all archived versions — so a fresh proposal can be made. Use when a session is contaminated or stuck in an unrecoverable state.

```bash
spec reset app/Http/Controllers/HomeController.php
```

### `spec wait [<file>] <status> [--timeout <secs>]`

Blocks until a session reaches a target status, then exits. Designed for AI agents that need to pause until a specific state arrives. Default timeout is 30 seconds — exits with `STATUS: TIMEOUT` if nothing changes, so the agent can loop without hitting tool time limits.

Without a file, watches **all** sessions and prints the matching file when found:

```bash
spec wait STUCK              # watch all sessions — exits when any goes STUCK
spec wait STUCK --timeout 25 # same, 25s timeout then STATUS: TIMEOUT
spec wait app/Http/Controllers/HomeController.php LOCKED  # watch one file
```

The mediator uses this to start cold without knowing which file will need attention:

```bash
# loop until stuck, act, repeat
spec wait STUCK --timeout 25
# → STATUS: STUCK app/Http/Controllers/HomeController.php
```

### `spec watch <file>`

Blocks indefinitely and emits a `STATUS:` line whenever the session changes. Useful for human-readable monitoring in a dedicated terminal.

```bash
spec watch app/Http/Controllers/HomeController.php
# → STATUS: WAITING_FOR_REPLY
# → STATUS: STUCK
# → STATUS: LOCKED
```

### `spec queue add <file> "<intent>" [--after <task-id>...]`

Add a task to the work queue. The orchestrator uses this to break a high-level request into file-level tasks with explicit dependencies.

```bash
spec queue add app/Models/User.php "add email validation — return bool, no exceptions"
spec queue add app/Http/Controllers/AuthController.php "implement login using User model" \
  --after task_abc123
```

### `spec queue list`

Show all tasks and their current status. Automatically syncs task state with live sessions — a task is marked done when its session is locked and built.

```bash
spec queue list
# [ ] task_abc123   app/Models/User.php         PENDING
# [B] task_def456   app/Http/Controllers/...    PENDING (blocked on task_abc123)
# [✓] task_ghi789   app/Http/Controllers/...    DONE
```

### `spec queue done <task-id>`

Manually mark a task complete. Usually automatic (tasks complete when their session locks and is built), but available for manual override.

```bash
spec queue done task_abc123
```

### `spec next`

Claim the next unblocked, pending task from the queue for the current agent. Checks session state before handing out work — blocked tasks (whose dependencies aren't done yet) are skipped automatically.

```bash
SPEC_AGENT_ID=alice spec next
# → Task:   task_abc123
# → File:   app/Models/User.php
# → Intent: add email validation...
# → Run: SPEC_AGENT_ID=alice spec propose app/Models/User.php "..."
```

### `spec run <role> [name] --with <claude|codex>`

Launch Claude Code or Codex CLI as a specific spec role. Sets `SPEC_PROVIDER`, `SPEC_ROLE`, and (for agent roles) `SPEC_AGENT_ID` automatically, then replaces the current process with the AI tool.

```bash
spec run orchestrator --with claude      # Claude Code as orchestrator (loads the queue)
spec run agent alice  --with claude      # Claude Code as agent "alice"
spec run agent bob    --with codex       # Codex as agent "bob"
spec run mediator     --with claude      # Claude Code as mediator
spec run implementer  --with codex       # Codex as implementer
```

Valid roles: `agent`, `proposer`, `mediator`, `implementer`, `orchestrator`.

### `spec install-skills [--target <dir>]`

Installs agent skill files to the target directory. Defaults to `~/.codex/skills/` or `~/.claude/skills/` (auto-detected).

```bash
spec install-skills                        # auto-detect
spec install-skills --target ~/.claude/skills
```

## Environment Variables

| Variable | Required | Description |
|---|---|---|
| `SPEC_AGENT_ID` | **Yes** (for agent commands) | Stable agent identity. Must be set before `propose`, `respond`, `concede`, `agree`. |
| `SPEC_PROVIDER` | Yes | LLM provider. Overrides `.spec/config.json`. See providers table below. |
| `SPEC_API_KEY` | Depends | API key. Not required for `claudecode` or `codex` providers. |
| `SPEC_MODEL` | No | Model override (e.g. `claude-opus-4-7`, `gpt-4o`). Overrides `.spec/config.json`. |
| `SPEC_ROLE` | No | Role enforcement. Set automatically by `spec run`. Restricts which commands are allowed. |

### Providers

| Provider | `SPEC_PROVIDER` value | Key required |
|---|---|---|
| Anthropic | `anthropic` | `SPEC_API_KEY` or `ANTHROPIC_API_KEY` |
| OpenAI | `openai` | `SPEC_API_KEY` or `OPENAI_API_KEY` |
| Ollama (local) | `ollama` | No (set `OLLAMA_HOST` or defaults to `localhost:11434`) |
| Claude Code CLI | `claudecode` | No — uses your existing Claude subscription |
| Codex CLI | `codex` | No — uses your existing OpenAI subscription |

### SPEC_ROLE

When set, restricts which commands an agent can run. Set automatically by `spec run`.

| Role | Allowed commands |
|---|---|
| `agent` / `proposer` | `propose`, `respond`, `concede`, `agree` |
| `mediator` | `clarify`, `reframe` |
| `implementer` | `build`, `test`, `release`, `status` |
| `orchestrator` | `queue`, `next`, `status` |

### Key isolation

`SPEC_PROVIDER` and `SPEC_API_KEY` are deliberately separate from whatever keys your app uses:

```bash
# Your app's key — Spec ignores this
export ANTHROPIC_API_KEY=sk-prod-app-key

# Spec's own keys — completely isolated
export SPEC_PROVIDER=anthropic
export SPEC_API_KEY=sk-spec-billing-key
export SPEC_MODEL=claude-sonnet-4-6   # optional
```

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

Commands that don't involve an agent identity (`build`, `clarify`, `reframe`, `status`, `log`, `state`, `lessons`) are called as plain `spec` with no alias needed.

## Running Multiple Agents

The full system runs five roles in parallel. Start the long-running roles first (orchestrator, mediator, implementer), then let agents pull work from the queue.

**Tab 1 — Orchestrator** *(loads the queue from a request or backlog)*
```bash
spec run orchestrator --with claude
# → reads codebase, populates queue with ordered tasks
```

**Tab 2 — Alice** *(agent, pulls tasks automatically)*
```bash
export SPEC_AGENT_ID=alice
spec run agent alice --with claude
# → spec next → spec propose → spec respond/agree → loops
```

**Tab 3 — Bob** *(agent, pulls tasks automatically)*
```bash
export SPEC_AGENT_ID=bob
spec run agent bob --with claude
# → spec next → spec propose → spec respond/agree → loops
```

**Tab 4 — Mediator** *(starts cold, waits for any session to go STUCK)*
```bash
spec run mediator --with claude
# → spec wait STUCK --timeout 25 → spec clarify/reframe → loops
```

**Tab 5 — Implementer** *(waits for LOCKED, then builds)*
```bash
spec run implementer --with claude
# → spec wait LOCKED --timeout 25 → spec build → spec test → loops
```

The agents don't need to know which files to work on — they call `spec next` and the queue handles assignment, respecting dependencies so no agent starts a task before its prerequisites are done.

## MCP Server

`spec-mcp` is a standalone MCP server that exposes the full spec protocol over stdio. It lets AI agents (Claude Code, Codex, or any MCP-compatible client) use spec tools without shell access, and **pushes notifications** to subscribed agents when sessions change — no polling required.

### Install

```bash
cargo install --path spec-mcp
```

### Configure Claude Code

Add to your Claude Code MCP configuration (e.g. `~/.claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "spec": {
      "command": "spec-mcp",
      "args": []
    }
  }
}
```

`spec-mcp` defaults to `SPEC_PROVIDER=claudecode` when no provider is set — it runs inside Claude Code, so the `claude` CLI is always available. To use a different provider, set it in the `env` block:

```json
{
  "mcpServers": {
    "spec": {
      "command": "spec-mcp",
      "args": [],
      "env": {
        "SPEC_PROVIDER": "anthropic",
        "SPEC_API_KEY": "sk-..."
      }
    }
  }
}
```

### Tools

All `spec` commands are available as MCP tools:

| Tool | Description |
|---|---|
| `spec_state` | Machine-readable status — no LLM call |
| `spec_log` | Full session history |
| `spec_status` | Overall project status |
| `spec_propose` | Propose a spec change |
| `spec_respond` | Respond to a proposal |
| `spec_concede` | Update your position |
| `spec_agree` | Sign off (supports `solo: true`) |
| `spec_clarify` | Mediator: surface contradictions |
| `spec_reframe` | Mediator: find common ground |
| `spec_build` | Implementer: write code from spec |

Agent identity is passed as a tool parameter (`agent_id`) rather than an environment variable:

```json
{
  "tool": "spec_propose",
  "arguments": {
    "file": "app/Http/Controllers/HomeController.php",
    "intent": "add JWT validation",
    "agent_id": "alice"
  }
}
```

### Resources

Sessions are exposed as subscribable MCP resources:

| URI | Content |
|---|---|
| `spec://session/<path>` | Session JSON for a given spec file |
| `spec://status` | Overall project status text |

### Push Notifications

Subscribe to a session resource to receive push notifications when it changes:

```json
{"method": "resources/subscribe", "params": {"uri": "spec://session/app/Http/Controllers/HomeController.php"}}
```

When another agent writes to that session, `spec-mcp` emits:

```json
{"method": "notifications/resources/updated", "params": {"uri": "spec://session/..."}}
```

The agent wakes up, calls `spec_state` (instant, no LLM) to see what changed, and decides its next move. No polling loop, no token burn.

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
- Consensus requires at least 2 distinct agents — one agent cannot lock a session alone (use `--solo` to bypass explicitly)
- `SPEC_AGENT_ID` must be set for all agent commands — no implicit identity
- The `.spec` file is plain markdown, written only at consensus
- After a mediator runs `clarify` or `reframe`, agents must `respond` or `concede` — new proposals are blocked
- The mediator never votes, never decides, never proposes
- The implementer never participates in resolution
- The orchestrator never proposes, responds, or agrees — it only queues
- Every message carries reasoning — proposals alone are not enough
- Queue tasks are auto-completed when their session locks and a build is recorded — no manual tracking needed

## Built With

Rust. The `spec` CLI has two dependencies: `serde`, `serde_json`. The `spec-mcp` server adds `tokio` and `notify`. LLM calls go over raw HTTP via `curl` — no SDKs.

Skills are embedded in the binary at compile time — `spec-agent`, `spec-proposer`, `spec-mediator`, `spec-implementer`, and `spec-orchestrator` are all installed via `spec install-skills`.
