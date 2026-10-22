# 0018. A release train on the stable branch

- Status: accepted (M5)
- Date: 2026-10-22

## Context

Every pull request ran the whole CI: Windows and macOS runners (`native-check`),
the desktop app built, started and tested end to end (`desktop`), on top of the
quick Linux checks. Those jobs are the slowest and the most costly, and they rarely
fail on a change that passes the Linux ones. The installers (`release.yml`) ran on
a `v*` tag pushed by hand and on pull requests that touched the packaging.

## Decision

- `main` is the development branch: pull requests go to it and run the quick checks
  of `ci.yml` (licenses, lint, tests with coverage, wasm, MSRV, the web front end,
  conformance), on Linux only.
- `stable` is a single release branch, a train: it receives `main` by a pull
  request merged with a merge commit (never squashed). There are no branches per
  version.
- `native.yml` (`native-check` on Windows and macOS, `desktop`) runs on pull
  requests to `stable`, every night on `main` (so a native failure is seen within a
  day, without blocking anyone) and by hand.
- `release.yml` runs on pull requests to `stable` and on pushes to it. Its `version`
  job checks that `Cargo.toml`, `tauri.conf.json` and the metainfo agree before the
  builds. A push to `stable` whose version has no `v<version>` tag tags the commit
  that was built and drafts the GitHub release with the installers; a maintainer
  publishes it. Tags are no longer pushed by hand.
- Fixes go to `main` and reach `stable` with the next train; when `main` cannot be
  released, a pull request to `stable` carries the fix and `stable` is merged back.

## Consequences

- Pull requests to `main` are faster and cheaper; a Windows, macOS or end-to-end
  failure shows up the next morning (or in the release pull request), not in the
  pull request that caused it.
- A packaging change is not built before it is merged to `main` unless someone runs
  `release.yml` by hand on its branch.
- The release is one merge: the version bump reaches `stable`, and the tag and the
  draft follow. The tag is pushed by the workflow because a draft release creates
  its tag only when published; a tag pushed with `GITHUB_TOKEN` starts no workflow,
  so the installers are not built twice.
- The repository settings must match: `stable` protected, with the jobs of the
  three workflows required and merge commits allowed; `main` no longer requires
  `native-check` or `desktop`.
