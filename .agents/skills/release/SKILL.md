---
name: release
description: Prepares and publishes Tau releases through the GitHub Actions Release workflow. Use when the user explicitly asks to release Tau, cut a patch/minor/major release, prepare a draft, or publish an approved draft.
disable-model-invocation: true
---

# Release Tau

Follow this flow only when explicitly invoked. Creating or editing this skill is not authorization to release. A release request authorizes the version-bump commit, normal push, workflow dispatch, and draft notes—not publication.

## Sources and boundaries

Read `AGENTS.md`, `docs/releases.md`, `.github/workflows/release.yml`, and `scripts/release-preflight.sh` before acting. Current repository code and docs are authoritative; stop and clarify if they conflict with this flow. Use `gh` for GitHub operations and resolve the repository from `origin`; pass `--repo "$REPO"` explicitly. Variables in commands below must be filled from verified state, not guessed.

This skill does not change release infrastructure, manage signing credentials, merge branches, fix unrelated code, force-push, overwrite tags/assets, or publish without approval. Never expose credentials. Do not use Pi's multi-agent workflow tool: “Release workflow” here means GitHub Actions `.github/workflows/release.yml`.

## 1. Preflight and choose a version

- Check GitHub authentication, `git status --short`, branch, remotes, and pending commits. Require a completely clean working tree, including untracked files. If dirty, stop and ask the user to commit or set aside changes; do not commit, stash, or discard them.
- Require local `main`. Fetch `origin` and tags without forcing updates. If behind, fast-forward only; if diverged or on another branch, stop rather than merge, rebase, or switch automatically.
- Inspect all local commits that will be pushed. Show their scope with the proposed release version before pushing; ask if anything appears unrelated or unexpected.
- Inspect existing releases (including drafts), tags, and active/pending Release runs. Do not dispatch a competing run. If an unfinished draft/run exists, ask whether to resume it or prepare a separate release; never silently publish, delete, or supersede it.
- Require the current versions in `package.json`, `src-tauri/tauri.conf.json`, and the root package in `src-tauri/Cargo.toml` / `src-tauri/Cargo.lock` to agree.
- Unless the user already specified `patch`, `minor`, or `major`, ask with all three resulting version numbers. Apply ordinary stable SemVer arithmetic: patch increments patch; minor increments minor and resets patch; major increments major and resets minor and patch. Do not infer a bump from the diff or make special exceptions for `0.x`.
- Set `VERSION` and `TAG=v<version>`. Verify neither the exact tag nor a release for it exists remotely. API errors are not evidence of absence. Stop on collisions rather than replacing anything.

For an explicit request to resume a draft, skip bumping and dispatching only after identifying its exact version, tag, successful workflow run, and commit. Continue with the verification and approval gates below; if provenance cannot be established, stop.

## 2. Bump and verify

Update only the version fields in:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml` (Tau's package, not dependencies)

Refresh `src-tauri/Cargo.lock` through Cargo, for example with `cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps`. Confirm its root Tau version updated; do not hand-edit dependency versions or run a broad dependency update. Inspect any other lockfile changes and stop on unexplained churn.

Run the full checks every time before committing and pushing:

```sh
bun run format
bun run lint
bun run build
bun run test
bun run test:e2e
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

`build` includes TypeScript checks. The real-Pi contract canary is not a default dependency. Do not substitute cached results or the CI production build for these checks. Report failed or unavailable checks and stop. Inspect auto-format changes; do not bundle unrelated formatting fixes into the release commit. Any code changes require rechecking the resulting tree.

Review the final version-only diff, stage explicit paths, and commit as `chore: release v<version>`. Record the full commit SHA as `RELEASE_SHA`. Ensure the tree is clean.

## 3. Push and dispatch

Push all intended local `main` commits, including the version bump, with a normal `git push origin main`. Never create or push the version tag here: an existing tag prevents the workflow from running.

Verify the remote `main` SHA equals `RELEASE_SHA` immediately before dispatch. Stop if it differs; do not dispatch against an unreviewed newer commit.

```sh
gh workflow run release.yml --repo "$REPO" --ref main
```

Identify the newly dispatched run by workflow, `workflow_dispatch` event, `main`, full `headSha == RELEASE_SHA`, and dispatch time/new run ID. Inspect pre-existing run IDs before dispatch when needed. Never blindly watch the latest run. If ambiguous, stop and resolve the exact run; if dispatch status is uncertain, look for the run before retrying.

```sh
gh run watch "$RUN_ID" --repo "$REPO" --exit-status
```

Wait for completion and require conclusion `success`. If environment approval is pending, tell the user; do not bypass it. On interruption, retain and re-query the same run ID instead of dispatching another run. Cancellation, timeout, or failure is not success.

## 4. Verify the draft and tag

After workflow success, query the release for `TAG` and verify:

- It exists and `isDraft` is true; “published as a draft” means created but **not publicly published**.
- Its title is `Tau <version>` and it is not a prerelease.
- It contains the single uploaded asset `tau-<version>-apple-silicon.dmg`, with nonzero size and completed upload state. GitHub's automatic source archives are not uploaded assets.
- The successful run built `RELEASE_SHA`.

The workflow already creates a lightweight `v<version>` tag immediately before the draft. Fulfill “tag the release commit” by verifying that remote tag targets exactly `RELEASE_SHA`, then fetching and checking it locally. For example, inspect `gh api "repos/$REPO/git/ref/tags/$TAG"`, require `object.type == commit` and `object.sha == RELEASE_SHA`, fetch without force, and check `git rev-parse "$TAG^{commit}"`.

Do not rely on a release's `target_commitish` or `--verify-tag` alone as proof of the commit. A missing/mismatched tag or non-draft release is a stop condition, not permission to create, move, or repair it automatically.

## 5. Write Features/Fixes notes

Read the bodies of the latest two or three **published** releases for style; exclude drafts. Identify the preceding published version in this release's ancestry, fetch its tag, and verify it is an ancestor of `RELEASE_SHA`. If there is no clear baseline, ask rather than guess. For a genuine first release, review history from the start.

Inspect both commits and the actual diff from that baseline to `RELEASE_SHA` (not the current working tree or a later `main`). Use commit messages as a guide, not the sole source of truth. Write concise, user-facing bullets describing shipped behavior, grouped into:

```md
### Features

- ...

### Fixes

- ...
```

Match previous releases' brevity, capitalization, and grouping. Omit empty sections. Exclude routine version bumps, internal refactors, tests, CI, and skill/docs changes unless they have a clear user-facing effect. Do not invent benefits or claim fixes unsupported by the diff. Flag breaking changes or required migration steps prominently when present. If there are no user-facing changes, use a short accurate maintenance note rather than fictional features/fixes.

Write notes to a temporary Markdown file outside the repository, then update the draft:

```sh
gh release edit "$TAG" --repo "$REPO" --notes-file "$NOTES_FILE"
```

Read back the body and draft status. Preserve any existing manually authored notes when resuming; propose changes instead of silently replacing them.

## 6. Explicit publication approval

Present the version, exact commit, successful run URL, draft URL, asset name, and complete proposed release notes. Report check results and the distribution smoke-test status from `docs/releases.md`. Ask the user to perform/confirm that manual smoke test; never claim it passed based on CI signing checks alone.

Then ask explicitly: **“Publish Tau <version> with these release notes?”** Leave it as a draft while awaiting an answer. Selecting a bump, asking to release, approving a push, or silence does not count. Approval applies only to the specific version, commit, and notes just shown; material changes require renewed approval. Do not publish with an unconfirmed required smoke test.

Only after explicit approval, recheck draft status, exact tag SHA, asset, successful run, and approved body. If anything changed, stop and ask again. Then:

```sh
gh release edit "$TAG" --repo "$REPO" --draft=false
```

Query it again and require `isDraft == false` with a publication timestamp. Return the release URL and version. Publication follows repository visibility; do not imply a private repository's release is publicly downloadable.

## Failures and partial runs

Stop on failed checks, push failure, unsuccessful workflow, missing assets, inconsistent provenance, or GitHub API errors. Report the exact stage, run URL, and any existing draft/tag; do not claim completion or blindly retry. Local version commits or a successful push do not authorize further repair actions.

A workflow may create its tag and then fail to create/upload the draft. Inspect both before proposing recovery. Follow `docs/releases.md`: incomplete draft/tag deletion requires explicit permission, and manual completion requires the exact verified artifact. Never delete or retarget a published release to reuse a version. Resume verified existing state instead of bumping again or starting duplicate runs.
