# Contributing to Schnipsle

Thank you for helping. This page describes how changes get into the project.

## Before you start

- Set up your machine with [docs/dev-setup.md](docs/dev-setup.md).
- For anything larger than a small fix, open an issue first so we can agree on the approach.
- Significant design choices need an [Architecture Decision Record](docs/adr/README.md). Changes to the vault format need an update of [docs/format.md](docs/format.md), an ADR and conformance fixtures.

## Workflow

1. Fork or branch from `main`; keep branches short-lived and focused.
2. Write tests with the change: unit tests for logic, golden files in `tests/fixtures/` for format, import and export behaviour.
3. Run the checks locally (the git hooks run the fast ones):

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   pnpm check
   cargo deny check
   ```

4. Open a pull request using the template. CI must be green before merge.
5. Note user-visible changes in [CHANGELOG.md](CHANGELOG.md) under "Unreleased".

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `type(scope): summary`, e.g.

```text
feat(core): extract hashtags from note bodies
fix(ui): keep cursor position after autosave
docs(adr): record music notation decision
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`. Scopes are usually `core`, `import`, `export`, `app`, `ui`, `format`, `adr`, `deps`.

## Sign-off

Every commit must carry a `Signed-off-by` line, which certifies the [Developer Certificate of Origin 1.1](https://developercertificate.org/): you wrote the change or have the right to submit it under the project's licence (Apache-2.0).

```sh
git commit -s -m "feat(core): …"
```

There is no contributor licence agreement.

## Coding standards

- **Rust:** `rustfmt`; `clippy` (including `pedantic`) with warnings as errors; no `unsafe`; public items documented; errors as typed `Result`s, no panics in library code except for broken invariants.
- **TypeScript:** `strict` type checking, ESLint (`strictTypeChecked`), Prettier.
- **Files:** every source file starts with `SPDX-License-Identifier: Apache-2.0`.
- **Dependencies:** licences must pass `cargo deny check` and `scripts/check-npm-licenses.mjs` (no GPL, AGPL, LGPL or SSPL; see ADR-0005).
- **Data safety:** any code that writes vault files uses atomic writes and never discards user text silently (format.md §10).

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
