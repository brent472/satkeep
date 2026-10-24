**Draft, not posted.** A bug report for [wizardsardine/liana](https://github.com/wizardsardine/liana), for the owner to post or not. Nothing here has been sent to Liana.

---

# `liana-cli` can block forever on a reply that has fully arrived

## What happens

Now and then, `liana-cli` hangs after `lianad` has handled the request and written the whole reply.
- **Where it waits:** the process sits in a blocking read on the socket (`unix_stream_read_generic`, state `S`, no CPU used) until it's killed.
- **`lianad` is fine:** it answers other clients instantly. The handler for the stuck connection is just waiting for the next request on it, and ends only when the client is killed.
- **Side effect:** for `getnewaddress`, the address has already been taken (`receive_index` has moved on).

**How often:** in our CI, on GitHub's `ubuntu-24.04` runners (2 vCPUs), 5 of 30 runs of a script making about 15 `liana-cli` calls hung. Every one hung in `getnewaddress`, whose reply is about 148 bytes. All were Liana v15.0, on regtest, with `poll_interval_secs = 1`.

## Why

[`lianad/src/bin/cli.rs`](https://github.com/wizardsardine/liana/blob/v15.0/lianad/src/bin/cli.rs) at v15.0 (unchanged on `master` as of 2026-10-24):

- **Line 117:** the reply buffer starts at 256 bytes: `let mut raw_response = vec![0; 256];`
- **Lines 139–142:** if a read fills the buffer exactly, the loop doubles the buffer and reads again **without trying to parse**:
  ```rust
  if total_read == raw_response.len() {
      raw_response.resize(2 * total_read, 0);
      continue;
  }
  ```
- **Line 145:** otherwise it calls `trimmed()` (lines 97–110), which **truncates the buffer to the bytes read so far**. If parsing fails because the reply is still partial, it goes round again (line 162, `Err(_) => continue`).
- **The server side,** [`lianad/src/jsonrpc/server/unix.rs` line 114](https://github.com/wizardsardine/liana/blob/v15.0/lianad/src/jsonrpc/server/unix.rs#L114): `lianad` writes the reply with `serde_json::to_writer` straight onto the unbuffered `UnixStream`, so it goes out in many small writes. It sends no newline after it and keeps the connection open for the next request.

**How they combine:**
1. The client reads part of a reply, T bytes. The buffer is truncated to T. Parsing fails, so it loops.
2. The read into the now-empty tail returns 0, `total_read == len` is true, and the buffer grows to 2T.
3. If the rest of the reply is exactly T bytes, that read fills the buffer exactly, so lines 139–142 grow it again and read without parsing.
4. The reply is complete and the server is waiting for the next request, so the read blocks forever.

In general it hangs whenever the read that completes the reply also fills the buffer exactly. For a 148-byte reply, that means a first read of 74 or 37 bytes. Because the server writes token by token, where the first read ends is down to timing, so it's intermittent.

**A second, smaller issue in the same loop:** if `lianad` closes the connection before sending anything, the first read returns 0. `trimmed()` then makes the buffer empty, and every later read into an empty slice returns 0 at once, so the client spins at 100% CPU instead of reporting the closed connection.

## Reproduction

The loop ported line for line to Python, with a fake server that sends one 148-byte reply in two writes and then keeps the connection open, as `lianad` does:

```python
# liana-cli v15.0's read loop (lianad/src/bin/cli.rs, main), ported line for
# line, against a server that sends one reply in two writes and then keeps
# the connection open for the next request, as lianad does.
import json, socket, threading, time

REPLY = json.dumps({"jsonrpc": "2.0", "result": {"address": "bcrt1p" + "q" * 58, "derivation_index": 4},
                    "id": "liana-cli-6317"}, separators=(",", ":")).encode()

def trimmed(buf, n):
    buf = buf[:n]
    while buf and 0x0a <= buf[-1] <= 0x0d:
        buf = buf[:-1]
    return buf

def client(sock):
    raw, total = bytearray(256), 0
    while True:
        with memoryview(raw) as whole, whole[total:] as view:
            n = sock.recv_into(view) if len(view) else 0  # read() into an empty slice returns 0
        total += n
        if total == len(raw):                              # "buffer full": assume more is coming
            raw.extend(bytes(total))                       # resize(2 * total_read)
            continue
        raw = bytearray(trimmed(bytes(raw), total))
        try:
            return json.loads(raw)                         # serde_json::from_slice, id check
        except ValueError:
            continue

def attempt(first):
    a, b = socket.socketpair()
    a.settimeout(3)
    def server():
        b.sendall(REPLY[:first]); time.sleep(0.3); b.sendall(REPLY[first:])
        time.sleep(5); b.close()                           # keeps the connection open, like lianad
    threading.Thread(target=server, daemon=True).start()
    time.sleep(0.1)                                        # the client reads mid-reply
    try:
        client(a); return "answered"
    except socket.timeout:
        return "HANGS (blocked in read)"
    finally:
        a.close()

L = len(REPLY)
print(f"reply is {L} bytes")
for first in (L // 2 - 1, L // 2, L // 2 + 1, 40):
    print(f"split {first} + {L - first}: {attempt(first)}")
```

Output:

```text
reply is 148 bytes
split 73 + 75: answered
split 74 + 74: HANGS (blocked in read)
split 75 + 73: answered
split 40 + 108: answered
```

## Suggested fix

Keep the bytes read so far, try to parse after every read, and treat end-of-file as an error. For example:

```rust
let mut raw_response = Vec::new();
let mut chunk = [0u8; 4096];
loop {
    let n = socket.read(&mut chunk).unwrap_or_else(|e| {
        eprintln!("Reading from {:?}: '{}'", &socket_file, e);
        process::exit(1);
    });
    if n == 0 {
        eprintln!("lianad closed the connection before a complete response");
        process::exit(1);
    }
    raw_response.extend_from_slice(&chunk[..n]);
    match serde_json::from_slice::<Json>(&raw_response) {
        Ok(response) if response.get("id") == request.get("id") => {
            // print as today
            return;
        }
        Ok(_) => { /* not our response: as today */ }
        Err(e) if e.is_eof() => continue, // incomplete: read more
        Err(e) => {
            eprintln!("Invalid response from lianad: '{e}'");
            process::exit(1);
        }
    }
}
```

**Optionally,** `lianad` could end each response with a newline, as clients already end each request. Then any client could read up to the newline, and `trimmed()` would be unnecessary. That changes the protocol, though, so the client-side fix alone may be preferable.

**Workaround we use:** our exit drill talks to `lianad`'s socket directly. It sends the request plus a newline, then reads until the bytes decode as one complete JSON object with the matching id (Python's `json.JSONDecoder.raw_decode`). RESULTS_PLACEHOLDER
