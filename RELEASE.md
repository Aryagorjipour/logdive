# Release runbook

Steps to cut a new logdive release. Targeted at the project maintainer.

## One-time setup

You'll need:

- Push access to the main branch of this repository.
- A [crates.io account](https://crates.io/) with a valid API token.
- Run `cargo login <your-token>` once per machine.

## Release procedure

### 1. Prepare the version

- Decide the new version number following [semver](https://semver.org/).
  - Breaking API change in `logdive-core` → major bump.
  - New feature, backwards-compatible → minor bump.
  - Bug fix only → patch bump.
- Update `version` in the workspace root `Cargo.toml` under `[workspace.package]`.
- Run `cargo build --workspace` so the lockfile updates.
- Update `CHANGELOG.md`:
  - Promote the `[Unreleased]` heading to `[X.Y.Z] - YYYY-MM-DD` (today's date).
  - Add a fresh empty `[Unreleased]` section above it.
  - Update the link references at the bottom.
- Commit the changes:

  ```bash
  git add -A
  git commit -m "release: prepare vX.Y.Z"
  git push origin main
  ```

### 2. Run pre-release checks

```bash
sh scripts/prerelease-check.sh
```

This runs the full battery: clean working tree, build, tests, clippy, fmt, binary size check, and `cargo publish --dry-run` for each of the three crates. **Do not proceed if anything fails.**

A note on the dry-run steps: `logdive-core` is fully verified (packaged and rebuilt in isolation), but `logdive` and `logdive-api` are checked with `--no-verify`. This is because those two crates depend on `logdive-core`, and the full verify step would try to resolve that dependency against crates.io. Before the first publish, that lookup fails because `logdive-core` isn't there yet. The `--no-verify` path still validates manifest shape, required fields, and packaged file set — the rebuild check just runs for real when we actually publish `logdive` and `logdive-api` (step 4 below).

### 3. Tag and push

```bash
VERSION=X.Y.Z
git tag -a v$VERSION -m "v$VERSION"
git push origin main v$VERSION
```

The tag push triggers `.github/workflows/release.yml`, which builds release binaries for Linux x86_64 and macOS arm64, packages them as tarballs, and uploads them to a new GitHub Release.

Watch the workflow run in the [Actions tab](https://github.com/Aryagorjipour/logdive/actions). Typical runtime: ~10 minutes.

### 4. Publish to crates.io

Publish in dependency order. Wait ~30 seconds between each step — the crates.io index needs time to propagate before the next crate can reference the newly published one.

```bash
cargo publish -p logdive-core
# wait ~30s
cargo publish -p logdive
# wait ~30s
cargo publish -p logdive-api
```

Verify each crate appears on its crates.io page before moving on:

- <https://crates.io/crates/logdive-core>
- <https://crates.io/crates/logdive>
- <https://crates.io/crates/logdive-api>

### 5. Post-release verification

- [ ] GitHub Release at `v$VERSION` has both platform tarballs uploaded.
- [ ] Release notes on the GitHub Release link to the CHANGELOG.
- [ ] All three crates visible on crates.io.
- [ ] `cargo install logdive logdive-api` from a clean machine succeeds.
- [ ] The installed binaries run without error (`logdive --version`, `logdive-api --version`).

## If something goes wrong

### Tagged a broken commit

```bash
git tag -d v$VERSION
git push --delete origin v$VERSION
```

Delete the GitHub Release manually via the web UI (tags and releases are separate objects). Fix the issue, commit, then re-tag.

### Published a broken crate to crates.io

You cannot delete a published version. You can yank it so no new projects pick it up, while existing `Cargo.lock` pins remain functional:

```bash
cargo yank --version X.Y.Z -p <crate-name>
```

Then publish a fixed version with an incremented patch number. See the [crates.io yank docs](https://doc.rust-lang.org/cargo/commands/cargo-yank.html).

### The release workflow fails mid-run

The workflow is idempotent per-artifact: re-running the failed job will re-upload the same tarball over the existing one. Use the "Re-run failed jobs" button in the Actions tab.

If the matrix job itself has a bug (not a transient issue), fix it on main, then delete and re-push the tag to re-trigger the workflow.
