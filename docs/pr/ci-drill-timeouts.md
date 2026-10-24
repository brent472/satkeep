Makes the exit drill fail fast instead of hanging, finds why it hung, and fixes that. The hang was a read bug in `liana-cli` v15.0. The drill now talks to `lianad`'s JSON-RPC socket directly, and its sync check is real. Test networks only.

## Timeouts

- **Each command** the drill runs (`bitcoind`, `bitcoin-cli`, Liana's signer, `satkeep`), and each request to `lianad`, gets 60 seconds (`DRILL_COMMAND_TIMEOUT`).
- **On a timeout,** the drill fails with only the command's name and subcommand, for example `lianad getnewaddress didn't finish within 60 s`. It never shows the arguments, input or output, which could hold key material.
- **The waits** for the node to start and for Liana to catch up have the same deadline. If Bitcoin Core never answers `getblockcount`, the drill raises "bitcoind didn't answer within 60 s".
- **Cleanup:** `lianad` is killed if it ignores the stop signal, and a hung `bitcoin-cli stop` only prints a warning.
- **The workflow's drill step** has a 15-minute backstop.

## Diagnostics on failure

While `lianad` and `bitcoind` are still running, the drill prints:
- the last 60 lines of each `lianad` log and of Core's `debug.log`;
- `lianad`'s `getinfo` and `bitcoin-cli getrpcinfo`, 10 s each;
- whether each Core wallet is still scanning.

**What's in that output:** `bitcoind` runs with `-debug=rpc`, which logs method names, never their parameters. `lianad` is watch-only, and the scan-log job checks all of this output too.

## The hang: cause and fix

**How it was reproduced:** with a temporary `repeat` input running copies of the drill in parallel, 5 of 30 copies hung, all in `getnewaddress`. The input is removed in the last commit.

**What the diagnostics showed:**
- **Ruled out:** no rescan running, no Core wallet scanning, nothing in Core's RPC queue.
- **`lianad` was fine:** it answered at once, and its address index had moved on.
- **`liana-cli` was stuck:** blocked in a socket read (`unix_stream_read_generic`, no CPU used).

**The cause** is in `liana-cli`'s read loop (`lianad/src/bin/cli.rs`, lines 130–163 at v15.0, unchanged on master):
- after a partial read it shrinks its buffer to the bytes so far;
- if the next read fills that buffer exactly, it reads again without parsing;
- `lianad` sends no newline after a reply and keeps the connection open, so when those were the reply's last bytes, nothing more arrives.

A Python port of the loop reproduces it: a 148-byte reply split 74 + 74 hangs, while 73 + 75 and other splits answer.

**The fix,** in `drill.py`:
- **Talking to Liana:** each request goes straight to `lianad`'s socket, one connection per call. The drill sends the request plus a newline, then reads until the bytes decode as one complete JSON object with its id (`json.JSONDecoder.raw_decode`). An error from `lianad` still raises `LianaError` with Liana's message. It's still Liana's own daemon and documented interface, and the Liana app people run doesn't use `liana-cli`.
- **A real sync check:** a block is mined before Liana starts, and `wait_for_liana_sync` needs Liana's `sync` at 1.0 as well as its height at the tip. Before, at block 0, the height check passed on its own while `sync` was 0.

**Result:** with both fixes, [20 of 20 copies passed](https://github.com/brent472/satkeep/actions/runs/40113389616), including 100 `getnewaddress` calls, and scan-log passed on all 20. Under the old one-in-six rate, 20 clean copies would happen by luck about 3% of the time.

**Locally,** the new client was tested against a fake `lianad` socket:
- **Replies that arrive:** split 74 + 74, one byte at a time, or all at once, each returns the reply.
- **Failures:** an error reply raises `LianaError`, a closed connection and a reply to another request fail, and a server that never answers times out in 2 s with no parameter in the message.

**Docs:**
- `docs/exit-drill.md`, new section "Talking to Liana", says why. Its by-hand steps keep `liana-cli`, with a note about the bug.
- `docs/pr/liana-cli-read-hang.md` is a **draft** bug report for Liana: what happens, the exact lines, the reproduction and a suggested fix. It has **not** been posted.

## Security review

None. Reviewed:
- **Error text:** errors and timeouts name only the method or command, never parameters, input or output. Key imports still go through stdin, which is never printed.
- **The socket client** sends the same requests `liana-cli` did. It passes strings as JSON strings rather than guessing whether they're JSON, which is stricter.
- **Diagnostics:** watch-only `lianad` output, RPC method names from `debug.log`, and wallet names with their scan status. The scan-log job checks all of it.
- **Removed before merge:** the temporary instrumentation (process-state record, trace logging, the `repeat` input and matrix).
- **No new dependencies.** The client uses only Python's standard library (`socket`, `json`, `itertools`).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
