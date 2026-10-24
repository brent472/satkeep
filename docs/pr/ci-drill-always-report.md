The exit drill's `drill` job now reports on every pull request, so it can be made a required check without blocking PRs it has nothing to do with.

## What changes

- **Triggers:**
  - every pull request, with no path filter;
  - every push to `main` (new);
  - the weekly run and manual runs, as before.
- **A first step, "Decide whether the drill runs", on pull requests only:**
  - **How:** it lists the PR's changes with `git diff --name-only` against the merge base with the PR's base branch, so it isn't limited by the capped API file list. The checkout now fetches the full history for this.
  - **Full drill** if anything changed under `crates/`, `contrib/exit-drill/` or `.github/exit-drill/`, or in `Cargo.toml`, `Cargo.lock`, `rust-toolchain*` or `.github/workflows/exit-drill.yml`.
  - **Also counted:** `.github/scripts/`, because the drill downloads Bitcoin Core with `.github/scripts/fetch-bitcoin-core.sh` (when in doubt, run it).
  - **Otherwise:** every later step is skipped, the job passes, and its summary says "No drill paths changed: skipped (changed files: N, none used by the drill)".
  - **When in doubt it runs:** a missing merge base, a file list it can't get, or an empty list all run the full drill.
- **Pushes to `main`, the weekly run and manual runs** never take the skip. The step writes why the drill ran into the job summary, for example "Full drill: a push run always runs it" or "Full drill: drill paths changed: …".
- **The scan-log job** is skipped when the drill was, since nothing ran to scan. If the drill job fails before deciding, scan-log still runs.
- `docs/exit-drill.md` and the README say how it decides.

## Checks

- **Locally,** the decision step's script was run against real commits:
  - this branch (it changes the workflow): full drill;
  - a docs-only commit: skipped;
  - a new `rust-toolchain.toml`: full drill;
  - a `push` event: full drill;
  - an unknown commit (no merge base): full drill.
- **On GitHub:** this PR changes the workflow, so its own drill run is the full drill: RESULT_FULL
- **The skip path:** a throwaway docs-only PR, opened against this branch so it runs the new workflow, then closed without merging: RESULT_SKIP

## Security review

None.
- **Inputs:** the step reads `github.base_ref` and the PR's head SHA through environment variables, never inline in the script, so a branch name can't inject commands.
- **Defaults:** every unexpected case runs the full drill.
- **Unchanged:** permissions are still `contents: read`, plus `actions: read` for scan-log. No secrets are used.
- **No new actions or dependencies.**

🤖 Generated with [Claude Code](https://claude.com/claude-code)
