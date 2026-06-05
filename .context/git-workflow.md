# git-workflow.md

## Branch strategy

```
main                         ← production; only receives merge commits from release branches
release/vX.Y.Z               ← integration branch per version series; squash-merges land here
feat/vX.Y.Z/<slug>           ← feature milestone branch, e.g. feat/v0.4.0/yaml-output
chore/vX.Y.Z/<slug>          ← non-feature milestone branch, e.g. chore/v0.4.0/bench-suite
fix/vX.Y.Z/<slug>            ← bug fix branch
```

**Flow for a milestone:**
1. `git checkout main && git pull`
2. `git checkout -b feat/v0.4.0/<slug>`
3. Implement, commit (one concern per commit)
4. Open PR targeting `release/v0.4.0`
5. Squash-merge
6. After all milestones: open PR `release/v0.4.0` → `main` (merge commit, not squash)

## Commit format

Conventional Commits. Types observed in this repo's history:

| Type | Use case |
|---|---|
| `feat:` | New user-visible feature |
| `fix:` | Bug fix |
| `chore:` | Tooling, CI, release infrastructure, CHANGELOG |
| `docs:` | Documentation only |
| `test:` | Tests only (no production code change) |
| `refactor:` | Code restructure, no behavior change |
| `milestone(N):` | Legacy — used before v0.2.0; not the current format |

**Milestone convention** (current): `chore(vX.Y.Z): <slug> — <description>`
or `feat(vX.Y.Z): <slug> — <description>`.

Examples from git log:
```
feat(v0.3.0): paren-queries — Clause::Group in AST, parser, executor
feat(v0.3.0): cli-query-flags — --output rename, --offset, QueryOptions
feat(v0.3.0): api-pagination — ?offset= param on GET /query
feat(v0.3.0): generated-columns — case-insensitive level via expression index
chore(v0.3.0): distroless — swap runtime image, --health-check flag
chore(v0.3.0): bump version to 0.3.0, update CHANGELOG
chore(v0.2.1): H1–H5 — security tests, functional tests, supply-chain hardening, docs, release (#9)
release: logdive v0.2.0 (#8)
```

Subject line ≤72 characters. Body only when the "why" isn't in the diff.

## Release process

Step-by-step to ship a version (taken from `scripts/prerelease-check.sh`):

1. All milestone PRs merged to `release/vX.Y.Z`.
2. Move `[Unreleased]` section in CHANGELOG.md to `[X.Y.Z] - YYYY-MM-DD`.
3. Bump `version` in root `Cargo.toml` `[workspace.package]` to `"X.Y.Z"`.
4. Run `./scripts/prerelease-check.sh` — must pass all 11 steps:
   - Clean working tree
   - On main branch (warning if not)
   - Tag does not already exist
   - `cargo build --workspace --release`
   - `cargo test --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo fmt --all --check`
   - `cargo deny check` (installs cargo-deny if missing)
   - Binary size check via `scripts/check-binary-size.sh` (both < 10 MB)
   - `cargo publish --dry-run --workspace` (Cargo ≥1.90) or just core (older)
   - CHANGELOG has a dated entry for this version
5. Merge `release/vX.Y.Z` → `main` (merge commit, not squash).
6. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z"`
7. Push: `git push origin main vX.Y.Z`
8. GitHub Actions `release.yml` builds prebuilt binaries and attaches them.
9. Publish to crates.io **in dependency order**:
   ```
   cargo publish -p logdive-core
   # wait ~30 seconds for index propagation
   cargo publish -p logdive
   # wait ~30 seconds
   cargo publish -p logdive-api
   ```

## What not to do

- **Never force-push main.** Use `--force-with-lease` only on feature/chore
  branches that you own.
- **Never commit directly to main or release/vX.Y.Z.** All code arrives via
  PR squash-merges.
- **Never skip `cargo deny check` before publishing.** The supply-chain policy
  is merge-blocking in CI; shipping a version that doesn't pass it would
  immediately fail the audit workflow.
- **Never publish without a dry-run first.** `cargo publish --dry-run` catches
  missing files and broken path deps before the irreversible publish.
- **Never publish out of dependency order.** Publishing `logdive` before
  `logdive-core` is on crates.io will fail because Cargo resolves the path dep
  as a registry dep when packaging.
- **Never create a tag before merging to main.** The prerelease check script
  validates you're on main before tagging.
- **Never leave `[Unreleased]` undated.** The script checks for a dated
  `[X.Y.Z] - YYYY-MM-DD` entry and fails if it finds TBD or a missing date.
