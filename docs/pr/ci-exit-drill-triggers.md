The exit drill now runs on its own, so it can't break unnoticed again. It did after PRs #7 and #9: the minimum heir delay and the refresh window refused its short test settings, and nobody saw until PR #12 ran it by hand.

## When it runs

- **Every week:** Mondays at 06:17 UTC.
- **On any pull request that touches what it runs:**
  - the three paths asked for: `crates/satkeep-savings`, `crates/satkeep-cli` and `contrib/exit-drill`;
  - also the workflow itself and its pinned signing keys (`.github/exit-drill/`), and `Cargo.toml` / `Cargo.lock`. A dependency bump changes the `satkeep` binary the drill uses.
- **By hand,** as before.

A newer push to the same PR cancels a drill still running for it (`concurrency`). This PR touches the workflow, so the drill runs on it: that's the check that the trigger works.

## Things to know

- **Failed scheduled runs:** GitHub emails the workflow's owner.
- **Paused schedules:** GitHub turns schedules off in a repo with no activity for 60 days.
- **Not a required check yet.** To make a failing drill block merging, add *Exit drill / drill* to the branch protection rule for `main`. That's a repo setting, so it's yours to change.

`docs/exit-drill.md` and the README's Checks section say all this.

## Security review

None. Reviewed:
- **Trigger:** `pull_request`, not `pull_request_target`. Code from a fork runs with a read-only token, and no secrets are used. `permissions` are unchanged: `contents: read`, plus `actions: read` for the log scan.
- **Concurrency group:** it uses `github.ref` as a key, never in a shell command, so there's no injection path.
- **Downloads:** each run checks Liana and Bitcoin Core against the same pinned SHA256s and signing keys. There's no `actions/cache`, so nothing can be poisoned between runs.
- **Key material:** unchanged. Keys are made inside each run and thrown away, and the log scan still runs after every drill.

No new dependencies.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
