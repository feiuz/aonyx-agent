# Releasing Aonyx Agent

Aonyx Agent releases ship as static binaries for Linux x86_64, macOS x86_64 + arm64, and Windows x86_64.

## Prerequisites (one-time)

```bash
cargo install cargo-release
```

## Cutting a release

1. **Sanity-check** the working tree is clean and on `main`:
   ```bash
   git status
   git switch main
   git pull
   ```

2. **Run the full test suite locally** (`release.toml` enforces this as a pre-hook):
   ```bash
   cargo test --workspace --all-features
   ```

3. **Update `CHANGELOG.md`**: move items from `[Unreleased]` under a new `[X.Y.Z] — YYYY-MM-DD` heading.

4. **Bump + tag** with `cargo-release`:
   ```bash
   cargo release patch --execute    # 0.1.0 → 0.1.1 (bug fixes)
   cargo release minor --execute    # 0.1.0 → 0.2.0 (new features, backwards-compatible)
   cargo release major --execute    # 0.1.0 → 1.0.0 (breaking changes)
   ```
   `cargo-release` will:
   - bump the workspace `version`,
   - commit as `chore: release v<X.Y.Z>`,
   - create a signed annotated tag `v<X.Y.Z>`.

5. **Review** the commit and tag locally:
   ```bash
   git show HEAD
   git tag -v "v<X.Y.Z>"
   ```

6. **Push** to trigger the release pipeline:
   ```bash
   git push origin main
   git push origin "v<X.Y.Z>"
   ```

7. The [`release.yml`](../.github/workflows/release.yml) workflow builds binaries on each target, computes SHA-256 sums, and creates the GitHub Release with auto-generated release notes.

## Versioning policy

- **0.x.y** — pre-stable. Breaking changes allowed in minor bumps, called out in the changelog.
- **≥ 1.0.0** — semver strict.

## Re-triggering a release

If the workflow failed and a tag already exists, re-run it manually:

- GitHub UI: *Actions → Release → Run workflow → tag = `v<X.Y.Z>`*.
- This rebuilds and re-uploads artifacts on the same release.

## Manual release (without `cargo-release`)

If `cargo-release` is unavailable:

```bash
# 1. Edit Cargo.toml workspace.package.version
# 2. Edit CHANGELOG.md
git add Cargo.toml CHANGELOG.md
git commit -m "chore: release v0.1.1"
git tag -a v0.1.1 -m "Aonyx Agent v0.1.1"
git push origin main v0.1.1
```
