# Skills (Codex / Claude Code)

This repo includes ready-to-install agent skills in `skills/<skill-name>/SKILL.md`.

## Install into Codex CLI

Copy the skill folders into your Codex skills directory:

```bash
mkdir -p ~/.codex/skills
cp -R skills/* ~/.codex/skills/
```

## Install into Claude Code

Copy the skill folders into your Claude skills directory:

```bash
mkdir -p ~/.claude/skills
cp -R skills/* ~/.claude/skills/
```

## Included skills

- `spec-agent` — propose/respond/concede/agree in a spec consensus session
- `spec-proposer` — create precise, testable proposals (guardrails for correct builds)
- `spec-mediator` — clarify/reframe stuck sessions (neutral)
- `spec-implementer` — build/test/release after consensus locks
