#!/usr/bin/env python3
"""Exit drill: recover and spend a Twofold Savings wallet with no Twofold code.

Twofold is used only to make fresh test keys and export the wallet
descriptor. Everything after that is done by Liana (lianad + liana-cli) and
Bitcoin Core (bitcoind + bitcoin-cli) on a fresh regtest node:

  1. Liana imports the descriptor and shows the same addresses as Twofold
     and Bitcoin Core.
  2. Liana spends with 2 of 3 keys, with DEVICE alone after its delay and
     with HEIR alone after its delay, and refuses each delayed spend one
     block early. Liana's own signer (HotSigner, via twofold_sign.rs) signs
     with the recovery phrases in Twofold's key files.
  3. Bitcoin Core alone does the same spends, and the node refuses each
     delayed spend one block early. Core can't read a recovery phrase, so
     this script turns each phrase into a master key (tprv) with Python's
     standard library: BIP39 seed, then BIP32 master key. That's written
     from the BIPs, separately from Twofold's code and Liana's.

Secrets never reach the log: the phrases stay in Twofold's key files and go
to Liana's signer as a file path; the master keys go to bitcoin-cli on stdin
(never as arguments). Both are registered with GitHub Actions' log masking.
Run on a throwaway machine only.

Environment: BITCOIN_BIN (folder with bitcoind and bitcoin-cli), LIANAD,
LIANA_CLI, LIANA_SIGN (twofold_sign built from Liana's source), TWOFOLD
(the twofold binary), WORK (an empty scratch folder).
"""

import hashlib
import hmac
import json
import os
import re
import subprocess
import sys
import time
import unicodedata

BIN = os.environ["BITCOIN_BIN"]
LIANAD = os.environ["LIANAD"]
LIANA_CLI = os.environ["LIANA_CLI"]
LIANA_SIGN = os.environ["LIANA_SIGN"]
TWOFOLD = os.environ["TWOFOLD"]
WORK = os.environ["WORK"]

DEVICE_DELAY, HEIR_DELAY = 10, 20
DATADIR = os.path.join(WORK, "bitcoind")
COOKIE = os.path.join(DATADIR, "regtest", ".cookie")
LIANA_CONF = os.path.join(WORK, "lianad.toml")
NUMS_XONLY = "50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0"
SAT = 100_000_000

results = []  # (check, passed, detail) for the job summary


class Failed(Exception):
    pass


def check(name, passed, detail=""):
    results.append((name, passed, detail))
    print(("PASS  " if passed else "FAIL  ") + name + (f": {detail}" if detail else ""), flush=True)
    if not passed:
        raise Failed(name)


def run(args, stdin=None):
    p = subprocess.run(args, input=stdin, capture_output=True, text=True)
    return p.returncode, p.stdout.strip(), p.stderr.strip()


def cli(*args, wallet=None, stdin_args=None):
    """bitcoin-cli. `stdin_args` go on stdin (-stdin), one per line, so
    secrets never appear in the process list."""
    cmd = [os.path.join(BIN, "bitcoin-cli"), "-regtest", f"-datadir={DATADIR}"]
    if wallet:
        cmd.append(f"-rpcwallet={wallet}")
    if stdin_args is not None:
        cmd.append("-stdin")
    code, out, err = run(cmd + list(args), "\n".join(stdin_args) + "\n" if stdin_args is not None else None)
    if code != 0:
        raise RuntimeError(f"bitcoin-cli {args[0] if args else ''} failed: {err}")
    return out


def cli_json(*args, **kw):
    return json.loads(cli(*args, **kw))


def liana(method, *params):
    """liana-cli. Returns the result, or raises LianaError with Liana's message."""
    code, out, err = run([LIANA_CLI, "--conf", LIANA_CONF, method] + [p if isinstance(p, str) else json.dumps(p) for p in params])
    if code != 0:
        raise RuntimeError(f"liana-cli {method} failed: {err}")
    response = json.loads(out)
    if "error" in response:
        raise LianaError(response["error"].get("message", str(response["error"])))
    return response.get("result")


class LianaError(Exception):
    pass


def twofold(*args):
    code, out, err = run([TWOFOLD] + list(args))
    if code != 0:
        raise RuntimeError(f"twofold {args[0]} failed: {err}")
    return out


def height():
    return int(cli("getblockcount"))


def mine(n, address):
    cli("generatetoaddress", str(n), address)


def wait_for_liana_sync():
    tip = height()
    for _ in range(120):
        try:
            if liana("getinfo")["block_height"] >= tip:
                return
        except (RuntimeError, LianaError):
            pass
        time.sleep(0.5)
    raise RuntimeError("Liana didn't catch up with the node")


def mined_in_tip(txid):
    tip = cli("getbestblockhash")
    return txid in cli_json("getblock", tip)["tx"]


def describe_spend(txid):
    """The input's sequence and witness size: a 2-of-3 spend has 3 signature
    slots plus script and control block (5 items) and no delay; a delayed
    spend has 1 signature (3 items) and its delay as the sequence."""
    tx = cli_json("getrawtransaction", txid, "true", cli("getbestblockhash"))
    vin = tx["vin"][0]
    return vin["sequence"], len(vin["txinwitness"])


def signature_count(psbt):
    decoded = cli_json("decodepsbt", psbt)
    return sum(len(i.get("taproot_script_path_sigs", [])) for i in decoded["inputs"])


B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
SECP256K1_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141


def phrase_to_tprv(phrase):
    """A BIP39 phrase (no passphrase) to its BIP32 master key on the test
    network, from the BIPs alone: BIP39's seed is PBKDF2-HMAC-SHA512 of the
    NFKD phrase with salt "mnemonic", 2048 rounds; BIP32's master key is
    HMAC-SHA512 of the seed under "Bitcoin seed"."""
    words = unicodedata.normalize("NFKD", " ".join(phrase.split()))
    seed = hashlib.pbkdf2_hmac("sha512", words.encode(), b"mnemonic", 2048)
    digest = hmac.new(b"Bitcoin seed", seed, hashlib.sha512).digest()
    key, chain_code = digest[:32], digest[32:]
    if not 0 < int.from_bytes(key, "big") < SECP256K1_ORDER:
        raise RuntimeError("unusable master key")  # BIP32: astronomically unlikely
    # tprv: version, depth 0, parent fingerprint 0, child number 0, chain code, 0x00 || key.
    payload = bytes.fromhex("04358394") + bytes(9) + chain_code + b"\x00" + key
    payload += hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    n, text = int.from_bytes(payload, "big"), ""
    while n:
        n, r = divmod(n, 58)
        text = B58[r] + text
    return "1" * (len(payload) - len(payload.lstrip(b"\x00"))) + text


def sign(psbt, holder):
    """Bitcoin Core signs with one holder's key, without finalizing."""
    before = signature_count(psbt)
    signed = cli_json("walletprocesspsbt", psbt, "true", "DEFAULT", "true", "false", wallet=holder)["psbt"]
    if signature_count(signed) <= before:
        raise RuntimeError(f"{holder} added no signature in Bitcoin Core")
    return signed


def liana_sign(psbt, holder):
    """Liana's HotSigner signs with one holder's recovery phrase. The phrase
    stays in the key file; only its path is passed."""
    before = signature_count(psbt)
    code, out, err = run([LIANA_SIGN, key_file(holder)], psbt)
    if code != 0:
        raise RuntimeError(f"Liana's signer failed for {holder}: {err}")
    if signature_count(out) <= before:
        raise RuntimeError(f"{holder} added no signature with Liana's signer")
    return out


def key_file(holder):
    return os.path.join(WORK, "twofold", "regtest", "keys", f"{holder}.key")


def txid_of(psbt):
    return cli_json("decodepsbt", psbt)["tx"]["txid"]


def main():
    os.makedirs(DATADIR, exist_ok=True)
    code, _, err = run([os.path.join(BIN, "bitcoind"), "-regtest", f"-datadir={DATADIR}", "-daemon", "-listen=0", "-fallbackfee=0.0001"])
    if code != 0:
        raise RuntimeError(f"bitcoind didn't start: {err}")
    for _ in range(60):
        if os.path.exists(COOKIE) and run([os.path.join(BIN, "bitcoin-cli"), "-regtest", f"-datadir={DATADIR}", "getblockcount"])[0] == 0:
            break
        time.sleep(0.5)
    print(f"Bitcoin Core {cli_json('getnetworkinfo')['subversion']} on regtest, height {height()}")

    burn = cli_json("deriveaddresses", cli_json("getdescriptorinfo", f"rawtr({NUMS_XONLY})")["descriptor"])[0]

    # --- Twofold: fresh keys and the descriptor, nothing more ------------------
    os.environ["TWOFOLD_HOME"] = os.path.join(WORK, "twofold")
    os.environ["TWOFOLD_RPC_URL"] = "http://127.0.0.1:18443"
    os.environ["TWOFOLD_RPC_COOKIE"] = COOKIE
    holders = {}
    for name in ["phone", "device", "helper", "heir"]:
        out = twofold("keygen", "--name", name)
        public = [line.strip() for line in out.splitlines() if line.startswith("  ")]
        with open(key_file(name)) as f:
            phrase = f.read().strip()
        print(f"::add-mask::{phrase}", flush=True)
        if len(phrase.split()) != 12:
            raise RuntimeError(f"{name}'s key file doesn't hold a 12-word recovery phrase")
        secret = phrase_to_tprv(phrase)
        print(f"::add-mask::{secret}", flush=True)
        holders[name] = {"public": public, "secret": secret, "fp": public[0][1:9]}
    check("`twofold keygen` saves a 12-word recovery phrase for each holder", True)
    twofold(
        "create", "--device-delay", str(DEVICE_DELAY), "--heir-delay", str(HEIR_DELAY),
        "--checkin-interval", "5", "--checkin-margin", "2",
        "--phone", holders["phone"]["public"][0], "--device", holders["device"]["public"][0],
        "--device-r", holders["device"]["public"][1], "--helper", holders["helper"]["public"][0],
        "--heir", holders["heir"]["public"][0],
    )
    descriptor = twofold("export")
    receive_desc, change_desc = twofold("export", "--split").splitlines()
    print(f"Exported descriptor: {descriptor}")
    twofold_receive = [twofold("address") for _ in range(5)]

    # --- Bitcoin Core wallets: one watch-only, one signer per holder ----------
    cli("-named", "createwallet", "wallet_name=watch", "disable_private_keys=true", "blank=true")
    cli("importdescriptors", json.dumps([
        {"desc": receive_desc, "active": True, "internal": False, "range": [0, 200], "timestamp": "now"},
        {"desc": change_desc, "active": True, "internal": True, "range": [0, 200], "timestamp": "now"},
    ]), wallet="watch")
    body = descriptor.split("#")[0]
    key = re.compile(r"\[([0-9a-f]{8})/87'/1'/(\d+)'\]tpub[1-9A-HJ-NP-Za-km-z]+/<0;1>/\*")
    for name, h in holders.items():
        private = key.sub(lambda m: f"{h['secret']}/87h/1h/{m.group(2)}h/<0;1>/*" if m.group(1) == h["fp"] else m.group(0), body)
        imports = []
        for branch, internal in (("0", False), ("1", True)):
            single = private.replace("<0;1>", branch)
            checksum = cli_json("getdescriptorinfo", stdin_args=[single])["checksum"]
            imports.append({"desc": f"{single}#{checksum}", "active": True, "internal": internal, "range": [0, 200], "timestamp": "now"})
        cli("-named", "createwallet", f"wallet_name={name}", "blank=true")
        res = cli_json("importdescriptors", stdin_args=[json.dumps(imports)], wallet=name)
        if not all(r.get("success") for r in res):
            raise RuntimeError(f"importing {name}'s key into Bitcoin Core failed")  # no detail: it could echo the key
        # The master key made here must be the one behind Twofold's public keys.
        derived = cli_json("listdescriptors", wallet=name)["descriptors"]
        if not any(f"[{h['fp']}/87h/1h/0h]" in d["desc"] or f"[{h['fp']}/87'/1'/0']" in d["desc"] for d in derived):
            raise RuntimeError(f"Bitcoin Core's key for {name} isn't the one in Twofold's public key")
    check("Bitcoin Core's keys, made from each phrase by Python's standard library, match Twofold's public keys", True)

    # --- Liana imports the descriptor ------------------------------------------
    with open(LIANA_CONF, "w") as f:
        f.write(
            f'data_dir = "{os.path.join(WORK, "liana")}"\nlog_level = "info"\nmain_descriptor = "{descriptor}"\n\n'
            f'[bitcoin_config]\nnetwork = "regtest"\npoll_interval_secs = 1\n\n'
            f'[bitcoind_config]\naddr = "127.0.0.1:18443"\ncookie_path = "{COOKIE}"\n'
        )
    lianad_log = open(os.path.join(WORK, "lianad.log"), "w")
    lianad = subprocess.Popen([LIANAD, "--conf", LIANA_CONF], stdout=lianad_log, stderr=subprocess.STDOUT)
    try:
        wait_for_liana_sync()
        print(f"Liana {liana('getinfo')['version']} running on the exported descriptor")
        check("Liana v15.0 imports the descriptor from `twofold export`", True)

        listed = liana("listaddresses", 0, 5)["addresses"]
        liana_receive = [a["receive"] for a in listed]
        liana_change = [a["change"] for a in listed]
        core_receive = cli_json("deriveaddresses", receive_desc, "[0,4]")
        core_change = cli_json("deriveaddresses", change_desc, "[0,4]")
        check("Liana's first 5 receive addresses match `twofold address`", liana_receive == twofold_receive)
        check("Liana's first 5 receive and change addresses match Bitcoin Core's", (liana_receive, liana_change) == (core_receive, core_change))

        # --- Fund: one block reward to a Liana address, matured -----------------
        mine(1, liana("getnewaddress")["address"])
        mine(100, burn)
        wait_for_liana_sync()

        # --- Liana, 2 of 3: PHONE + HELPER. Also creates the coins for the rest -
        targets = {label: liana("getnewaddress")["address"] for label in
                   ["liana-device", "liana-heir", "core-2of3", "core-device", "core-heir"]}
        destinations = {burn: SAT, **{address: SAT for address in targets.values()}}
        created = liana("createspend", destinations, [], 2)
        if "psbt" not in created:
            raise RuntimeError(f"Liana couldn't create the spend: {created}")
        psbt = created["psbt"]
        for holder in ("phone", "helper"):
            psbt = liana_sign(psbt, holder)
        liana("updatespend", psbt)
        txid = txid_of(psbt)
        liana("broadcastspend", txid)
        mine(1, burn)
        wait_for_liana_sync()
        sequence, witness = describe_spend(txid)
        check("Liana spends with 2 of 3 keys (PHONE + HELPER, signed by Liana's HotSigner)",
              mined_in_tip(txid) and witness == 5 and sequence >= 0xFFFFFFFD, f"txid {txid}")
        funded_at = height()
        decoded = cli_json("getrawtransaction", txid, "true", cli("getbestblockhash"))
        outpoint = {label: f"{txid}:{next(o['n'] for o in decoded['vout'] if o['scriptPubKey'].get('address') == address)}"
                    for label, address in targets.items()}

        # --- Bitcoin Core alone, 2 of 3: PHONE + DEVICE -------------------------
        def core_psbt(label, sequence=None):
            txid_, vout = outpoint[label].split(":")
            spend_input = {"txid": txid_, "vout": int(vout)}
            if sequence is not None:
                spend_input["sequence"] = sequence
            return cli_json("walletcreatefundedpsbt", json.dumps([spend_input]), json.dumps([{burn: 0.5}]), "0",
                            json.dumps({"add_inputs": False, "fee_rate": 2}), "true", wallet="watch")["psbt"]

        def core_finalize(psbt_):
            final = cli_json("finalizepsbt", psbt_)
            if not final.get("complete"):
                raise RuntimeError("Bitcoin Core couldn't finalize the spend")
            return final["hex"]

        def node_accepts(hex_):
            r = cli_json("testmempoolaccept", json.dumps([hex_]))[0]
            return r["allowed"], r.get("reject-reason", "")

        psbt = core_psbt("core-2of3")
        for holder in ("phone", "device"):
            psbt = sign(psbt, holder)
        hex_ = core_finalize(psbt)
        txid = cli("sendrawtransaction", hex_)
        mine(1, burn)
        sequence, witness = describe_spend(txid)
        check("Bitcoin Core alone spends with 2 of 3 keys (PHONE + DEVICE)",
              mined_in_tip(txid) and witness == 5 and sequence >= 0xFFFFFFFD, f"txid {txid}")

        # --- Delayed paths, each refused one block early, then accepted ---------
        def age():
            return height() - funded_at + 1  # confirmations; a spend in the next block sees this many

        def age_to(n):
            mine(n - age(), burn)
            wait_for_liana_sync()

        for path, holder, delay in (("device", "device", DEVICE_DELAY), ("heir", "heir", HEIR_DELAY)):
            age_to(delay - 1)
            try:
                liana("createrecovery", burn, 2, delay, [outpoint[f"liana-{path}"]])
                check(f"Liana refuses {path.upper()} alone one block early ({delay - 1} of {delay} confirmations)", False, "it built the spend")
            except LianaError as e:
                check(f"Liana refuses {path.upper()} alone one block early ({delay - 1} of {delay} confirmations)", True, f'"{e}"')
            early = core_psbt(f"core-{path}", sequence=delay)
            early = sign(early, holder)
            allowed, reason = node_accepts(core_finalize(early))
            check(f"Bitcoin Core alone: the node refuses {path.upper()} alone one block early",
                  not allowed and "non-BIP68-final" in reason, f'"{reason}"')

            # Exactly at the delay: Liana's and Core's spends both go out, then one block.
            age_to(delay)
            psbt = liana("createrecovery", burn, 2, delay, [outpoint[f"liana-{path}"]])["psbt"]
            psbt = liana_sign(psbt, holder)
            liana("updatespend", psbt)
            liana_txid = txid_of(psbt)
            liana("broadcastspend", liana_txid)

            hex_ = core_finalize(sign(core_psbt(f"core-{path}", sequence=delay), holder))
            allowed, reason = node_accepts(hex_)
            core_txid = cli("sendrawtransaction", hex_) if allowed else None

            mine(1, burn)
            wait_for_liana_sync()
            check(f"Liana spends with {path.upper()} alone at exactly {delay} confirmations (signed by Liana's HotSigner)",
                  mined_in_tip(liana_txid) and describe_spend(liana_txid) == (delay, 3), f"txid {liana_txid}")
            check(f"Bitcoin Core alone spends with {path.upper()} alone at exactly {delay} confirmations",
                  bool(core_txid) and mined_in_tip(core_txid) and describe_spend(core_txid) == (delay, 3),
                  f"txid {core_txid}" if core_txid else f'"{reason}"')
    finally:
        lianad.terminate()
        lianad.wait(timeout=30)
        run([os.path.join(BIN, "bitcoin-cli"), "-regtest", f"-datadir={DATADIR}", "stop"])


def summary():
    lines = ["## Exit drill (regtest)", "", "| Check | Result | Detail |", "| --- | --- | --- |"]
    for name, passed, detail in results:
        lines.append(f"| {name} | {'✅' if passed else '❌'} | {detail.replace('|', '/')} |")
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a") as f:
            f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    status = 0
    try:
        main()
    except Failed:
        status = 1
    except Exception as e:  # report, but never with a traceback that could carry key text
        print(f"ERROR: {e}", flush=True)
        status = 1
    summary()
    sys.exit(status)
