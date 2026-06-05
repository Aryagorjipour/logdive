# .context/

AI context for the logdive project. Tracked in the repo (removed from
.gitignore on 2026-06-05). Written from a full cold-start exploration on
2026-06-01, updated through v0.3.0 on 2026-06-05.

Files in this directory are written from what actually exists in the code, not
from what was planned or assumed. If any file contradicts the current code, the
code wins; update or remove the offending section.

Update the relevant files after each milestone ships — stale context is worse
than no context because it misleads the next session into reasoning from wrong
premises.

## Files

| File | What it covers |
|---|---|
| `project.md` | Identity, target user, non-goals, maintainer notes |
| `architecture.md` | Crate map, schema DDL, query grammar, locked decisions, dependency inventory |
| `git-workflow.md` | Branch strategy, commit format, release process |
| `operating-rules.md` | Working rules (plan, compile checkpoint, naming, etc.) |
| `traps.md` | Known pitfalls and non-obvious constraints |
| `v0.2.x-summary.md` | Narrative summary of v0.2.0, v0.2.1, and v0.3.0 milestones |
