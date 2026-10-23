#!/usr/bin/env python3
"""Exit drill: recover and spend a Twofold Savings wallet with no Twofold code.

Twofold is used only to make fresh test keys and export the wallet
descriptor. Everything after that is done by Liana (lianad + liana-cli) and
Bitcoin Core (bitcoind + bitcoin-cli) on a fresh regtest node:

Savings comes in two shapes, and the drill covers both, with the same keys:

A. Savings with a beneficiary: 2 of 3, plus HEIR alone after the heir delay.
  1. Liana imports the descriptor and shows the same addresses as Twofold
     and Bitcoin Core.
  2. Liana spends with 2 of 3 keys, and with HEIR alone after its delay,
     and refuses the heir spend one block early. Liana's own signer
     (HotSigner, via twofold_sign.rs) signs with the recovery phrases in
     Twofold's key files.
  3. Bitcoin Core alone does the same spends, the node refuses the heir
     spend one block early, and DEVICE alone is refused at any age (there
     is no card-only path).
B. Savings with no beneficiary yet: plain 2 of 3, no delayed path.
  4. Whether Liana accepts this shape is recorded either way (Liana may
     require a delayed path); if it does, Liana spends with 2 of 3 keys.
  5. Bitcoin Core alone derives the same addresses, spends with 2 of 3
     keys, and refuses any single key.

Core can't read a recovery phrase, so
     this script turns each phrase into a master key (tprv) with Python's
     standard library: BIP39 seed, then BIP32 master key. That's written
     from the BIPs, separately from Twofold's code and Liana's.

Secrets are never printed, not even masked: the phrases stay in Twofold's
key files and go to Liana's signer as a file path; the master keys go to
bitcoin-cli on stdin (never as arguments). Any command that reads a key
fails without repeating its output. The workflow then scans this script's
output and the whole job log, and fails if either has anything key-like
(scan_log.py). Run on a throwaway machine only.

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

HEIR_DELAY = 20
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
    secrets never appear in the process list. Only key imports use them, so
    a failure then reports no detail: Core's message could echo the key."""
    cmd = [os.path.join(BIN, "bitcoin-cli"), "-regtest", f"-datadir={DATADIR}"]
    if wallet:
        cmd.append(f"-rpcwallet={wallet}")
    if stdin_args is not None:
        cmd.append("-stdin")
    code, out, err = run(cmd + list(args), "\n".join(stdin_args) + "\n" if stdin_args is not None else None)
    if code != 0:
        if stdin_args is not None:
            raise RuntimeError(f"bitcoin-cli {args[0]} failed (no detail: its input holds a key)")
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
        if args[0] == "keygen":
            raise RuntimeError("twofold keygen failed (no detail: it handles a new key)")
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
    code, out, _ = run([LIANA_SIGN, key_file(holder)], psbt)
    if code != 0:
        raise RuntimeError(f"Liana's signer failed for {holder} (no detail: it reads the key)")
    if signature_count(out) <= before:
        raise RuntimeError(f"{holder} added no signature with Liana's signer")
    return out


def key_file(holder):
    return os.path.join(WORK, "twofold", "regtest", "keys", f"{holder}.key")


def txid_of(psbt):
    return cli_json("decodepsbt", psbt)["tx"]["txid"]


def core_wallets(prefix, holders, descriptor):
    """A watch-only Bitcoin Core wallet for `descriptor`, and one signer
    wallet per holder with that holder's master key in place of its tpub.
    Returns the receive and change descriptors and the signer wallet names."""
    receive_desc, change_desc = descriptor_halves(descriptor)
    cli("-named", "createwallet", f"wallet_name={prefix}watch", "disable_private_keys=true", "blank=true")
    cli("importdescriptors", json.dumps([
        {"desc": receive_desc, "active": True, "internal": False, "range": [0, 200], "timestamp": "now"},
        {"desc": change_desc, "active": True, "internal": True, "range": [0, 200], "timestamp": "now"},
    ]), wallet=f"{prefix}watch")
    body = descriptor.split("#")[0]
    key = re.compile(r"\[([0-9a-f]{8})/87'/1'/(\d+)'\]tpub[1-9A-HJ-NP-Za-km-z]+/<0;1>/\*")
    signers = {}
    for name, h in holders.items():
        if f"[{h['fp']}/" not in body:
            continue  # HEIR has no key in Savings with no beneficiary
        private = key.sub(lambda m: f"{h['secret']}/87h/1h/{m.group(2)}h/<0;1>/*" if m.group(1) == h["fp"] else m.group(0), body)
        imports = []
        for branch, internal in (("0", False), ("1", True)):
            single = private.replace("<0;1>", branch)
            checksum = cli_json("getdescriptorinfo", stdin_args=[single])["checksum"]
            imports.append({"desc": f"{single}#{checksum}", "active": True, "internal": internal, "range": [0, 200], "timestamp": "now"})
        wallet = f"{prefix}{name}"
        cli("-named", "createwallet", f"wallet_name={wallet}", "blank=true")
        res = cli_json("importdescriptors", stdin_args=[json.dumps(imports)], wallet=wallet)
        if not all(r.get("success") for r in res):
            raise RuntimeError(f"importing {name}'s key into Bitcoin Core failed")  # no detail: it could echo the key
        # The master key made here must be the one behind Twofold's public keys.
        derived = cli_json("listdescriptors", wallet=wallet)["descriptors"]
        if not any(f"[{h['fp']}/87h/1h/0h]" in d["desc"] or f"[{h['fp']}/87'/1'/0']" in d["desc"] for d in derived):
            raise RuntimeError(f"Bitcoin Core's key for {name} isn't the one in Twofold's public key")
        signers[name] = wallet
    return receive_desc, change_desc, signers


def descriptor_halves(descriptor):
    """The receive and change descriptors, each with its own checksum."""
    body = descriptor.split("#")[0]
    halves = []
    for branch in ("0", "1"):
        single = body.replace("<0;1>", branch)
        halves.append(f"{single}#{cli_json('getdescriptorinfo', single)['checksum']}")
    return halves


def start_liana(name, descriptor):
    """Starts lianad on `descriptor`. Returns the process and its config path."""
    conf = os.path.join(WORK, f"{name}.toml")
    with open(conf, "w") as f:
        f.write(
            f'data_dir = "{os.path.join(WORK, name)}"\nlog_level = "info"\nmain_descriptor = "{descriptor}"\n\n'
            f'[bitcoin_config]\nnetwork = "regtest"\npoll_interval_secs = 1\n\n'
            f'[bitcoind_config]\naddr = "127.0.0.1:18443"\ncookie_path = "{COOKIE}"\n'
        )
    log = open(os.path.join(WORK, f"{name}.log"), "w")
    return subprocess.Popen([LIANAD, "--conf", conf], stdout=log, stderr=subprocess.STDOUT), conf


def stop(process):
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=30)


def main():
    global LIANA_CONF
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

    # --- Twofold: fresh keys and the two descriptors, nothing more -------------
    os.environ["TWOFOLD_HOME"] = os.path.join(WORK, "twofold")
    os.environ["TWOFOLD_RPC_URL"] = "http://127.0.0.1:18443"
    os.environ["TWOFOLD_RPC_COOKIE"] = COOKIE
    holders = {}
    for name in ["phone", "device", "helper", "heir"]:
        out = twofold("keygen", "--name", name)
        public = [line.strip() for line in out.splitlines() if line.startswith("  ")]
        with open(key_file(name)) as f:
            phrase = f.read().strip()
        if len(phrase.split()) != 12:
            raise RuntimeError(f"{name}'s key file doesn't hold a 12-word recovery phrase")
        secret = phrase_to_tprv(phrase)
        holders[name] = {"public": public, "secret": secret, "fp": public[0][1:9]}
    check("`twofold keygen` saves a 12-word recovery phrase for each holder", True)
    keys = ["--phone", holders["phone"]["public"][0], "--device", holders["device"]["public"][0],
            "--helper", holders["helper"]["public"][0]]
    twofold("create", *keys, "--heir", holders["heir"]["public"][0], "--heir-delay", str(HEIR_DELAY),
            "--checkin-interval", "10", "--checkin-margin", "5")
    descriptor = twofold("export")
    print(f"Exported descriptor, with a beneficiary: {descriptor}")
    twofold_receive = [twofold("address") for _ in range(5)]
    plain_dir = os.path.join(WORK, "twofold-plain")
    twofold("--wallet-dir", plain_dir, "create", *keys)
    plain_descriptor = twofold("--wallet-dir", plain_dir, "export")
    print(f"Exported descriptor, no beneficiary: {plain_descriptor}")
    plain_receive = [twofold("--wallet-dir", plain_dir, "address") for _ in range(5)]

    # --- Bitcoin Core wallets: watch-only and signers, for each shape ----------
    receive_desc, change_desc, signers = core_wallets("", holders, descriptor)
    plain_receive_desc, _, plain_signers = core_wallets("plain-", holders, plain_descriptor)
    check("Bitcoin Core's keys, made from each phrase by Python's standard library, match Twofold's public keys", True)

    def core_psbt(watch, outpoint_, sequence=None):
        txid_, vout = outpoint_.split(":")
        spend_input = {"txid": txid_, "vout": int(vout)}
        if sequence is not None:
            spend_input["sequence"] = sequence
        return cli_json("walletcreatefundedpsbt", json.dumps([spend_input]), json.dumps([{burn: 0.5}]), "0",
                        json.dumps({"add_inputs": False, "fee_rate": 2}), "true", wallet=watch)["psbt"]

    def core_finalize(psbt_):
        final = cli_json("finalizepsbt", psbt_)
        if not final.get("complete"):
            raise RuntimeError("Bitcoin Core couldn't finalize the spend")
        return final["hex"]

    def core_complete(psbt_):
        return cli_json("finalizepsbt", psbt_).get("complete", False)

    def node_accepts(hex_):
        r = cli_json("testmempoolaccept", json.dumps([hex_]))[0]
        return r["allowed"], r.get("reject-reason", "")

    # =========================== A. With a beneficiary ==========================
    lianad, LIANA_CONF = start_liana("liana", descriptor)
    try:
        wait_for_liana_sync()
        print(f"Liana {liana('getinfo')['version']} running on the exported descriptor")
        check("A. Liana v15.0 imports the descriptor with a beneficiary from `twofold export`", True)

        listed = liana("listaddresses", 0, 5)["addresses"]
        liana_receive = [a["receive"] for a in listed]
        liana_change = [a["change"] for a in listed]
        core_receive = cli_json("deriveaddresses", receive_desc, "[0,4]")
        core_change = cli_json("deriveaddresses", change_desc, "[0,4]")
        check("A. Liana's first 5 receive addresses match `twofold address`", liana_receive == twofold_receive)
        check("A. Liana's first 5 receive and change addresses match Bitcoin Core's", (liana_receive, liana_change) == (core_receive, core_change))

        # --- Fund: one block reward to Liana, one to plain Savings, matured ----
        mine(1, liana("getnewaddress")["address"])
        mine(1, plain_receive[0])
        mine(100, burn)
        wait_for_liana_sync()

        # --- Liana, 2 of 3: PHONE + HELPER. Also creates the coins for the rest -
        targets = {label: liana("getnewaddress")["address"] for label in ["liana-heir", "core-2of3", "core-heir", "core-device"]}
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
        check("A. Liana spends with 2 of 3 keys (PHONE + HELPER, signed by Liana's HotSigner)",
              mined_in_tip(txid) and witness == 5 and sequence >= 0xFFFFFFFD, f"txid {txid}")
        funded_at = height()
        decoded = cli_json("getrawtransaction", txid, "true", cli("getbestblockhash"))
        outpoint = {label: f"{txid}:{next(o['n'] for o in decoded['vout'] if o['scriptPubKey'].get('address') == address)}"
                    for label, address in targets.items()}

        # --- Bitcoin Core alone, 2 of 3: PHONE + DEVICE -------------------------
        psbt = core_psbt("watch", outpoint["core-2of3"])
        for holder in ("phone", "device"):
            psbt = sign(psbt, signers[holder])
        txid = cli("sendrawtransaction", core_finalize(psbt))
        mine(1, burn)
        sequence, witness = describe_spend(txid)
        check("A. Bitcoin Core alone spends with 2 of 3 keys (PHONE + DEVICE)",
              mined_in_tip(txid) and witness == 5 and sequence >= 0xFFFFFFFD, f"txid {txid}")

        # --- The heir path: refused one block early, then accepted -------------
        def age():
            return height() - funded_at + 1  # confirmations; a spend in the next block sees this many

        def age_to(n):
            mine(n - age(), burn)
            wait_for_liana_sync()

        delay = HEIR_DELAY
        age_to(delay - 1)
        try:
            liana("createrecovery", burn, 2, delay, [outpoint["liana-heir"]])
            check(f"A. Liana refuses HEIR alone one block early ({delay - 1} of {delay} confirmations)", False, "it built the spend")
        except LianaError as e:
            check(f"A. Liana refuses HEIR alone one block early ({delay - 1} of {delay} confirmations)", True, f'"{e}"')
        early = sign(core_psbt("watch", outpoint["core-heir"], sequence=delay), signers["heir"])
        allowed, reason = node_accepts(core_finalize(early))
        check("A. Bitcoin Core alone: the node refuses HEIR alone one block early",
              not allowed and "non-BIP68-final" in reason, f'"{reason}"')

        # Exactly at the delay: Liana's and Core's spends both go out, then one block.
        age_to(delay)
        psbt = liana("createrecovery", burn, 2, delay, [outpoint["liana-heir"]])["psbt"]
        psbt = liana_sign(psbt, "heir")
        liana("updatespend", psbt)
        liana_txid = txid_of(psbt)
        liana("broadcastspend", liana_txid)

        hex_ = core_finalize(sign(core_psbt("watch", outpoint["core-heir"], sequence=delay), signers["heir"]))
        allowed, reason = node_accepts(hex_)
        core_txid = cli("sendrawtransaction", hex_) if allowed else None

        mine(1, burn)
        wait_for_liana_sync()
        check(f"A. Liana spends with HEIR alone at exactly {delay} confirmations (signed by Liana's HotSigner)",
              mined_in_tip(liana_txid) and describe_spend(liana_txid) == (delay, 3), f"txid {liana_txid}")
        check(f"A. Bitcoin Core alone spends with HEIR alone at exactly {delay} confirmations",
              bool(core_txid) and mined_in_tip(core_txid) and describe_spend(core_txid) == (delay, 3),
              f"txid {core_txid}" if core_txid else f'"{reason}"')

        # --- DEVICE alone, past every delay: no path takes it --------------------
        alone = sign(core_psbt("watch", outpoint["core-device"]), signers["device"])
        check(f"A. DEVICE alone is refused at {age()} confirmations: there is no card-only path",
              not core_complete(alone), "Bitcoin Core can't complete the spend")
    finally:
        stop(lianad)

    # =========================== B. No beneficiary yet ==========================
    core_plain = cli_json("deriveaddresses", plain_receive_desc, "[0,4]")
    check("B. Bitcoin Core derives the same first 5 addresses as `twofold address` for Savings with no beneficiary",
          core_plain == plain_receive)

    lianad, LIANA_CONF = start_liana("liana-plain", plain_descriptor)
    try:
        accepted = False
        for _ in range(40):
            if lianad.poll() is not None:
                break
            try:
                liana("getinfo")
                accepted = True
                break
            except (RuntimeError, LianaError, ValueError):
                time.sleep(0.5)
        if accepted:
            wait_for_liana_sync()
            check("B. Liana v15.0 accepts Savings with no beneficiary", True, "recorded; Bitcoin Core is checked either way")
        else:
            with open(os.path.join(WORK, "liana-plain.log")) as f:
                lines = [line.strip() for line in f if "rror" in line or "escriptor" in line]
            # Shorten the descriptor Liana quotes, so its reason fits.
            reason = re.sub(r"tr\([^']*\)(#[0-9a-z]+)?", "tr(…)", lines[-1] if lines else "lianad exited")[:300]
            check("B. Liana v15.0 doesn't accept Savings with no beneficiary (recorded, not a failure)", True, f'"{reason}"')

        plain_coin = next(u for u in cli_json("listunspent", "1", "9999999", "[]", "true", wallet="plain-watch"))
        plain_outpoint = f"{plain_coin['txid']}:{plain_coin['vout']}"
        for holder in ("phone", "device", "helper"):
            alone = sign(core_psbt("plain-watch", plain_outpoint, sequence=HEIR_DELAY), plain_signers[holder])
            if core_complete(alone):
                check(f"B. {holder.upper()} alone is refused in Savings with no beneficiary", False, "Core completed it")
        check(f"B. Bitcoin Core alone: no single key spends Savings with no beneficiary, after {HEIR_DELAY}+ blocks", True)

        if accepted:
            # A fresh coin Liana watches from the start, matured.
            mine(1, liana("getnewaddress")["address"])
            mine(100, burn)
            wait_for_liana_sync()
            created = liana("createspend", {burn: SAT}, [], 2)
            if "psbt" not in created:
                raise RuntimeError(f"Liana couldn't create the spend: {created}")
            psbt = created["psbt"]
            for holder in ("phone", "helper"):
                psbt = liana_sign(psbt, holder)
            liana("updatespend", psbt)
            txid = txid_of(psbt)
            liana("broadcastspend", txid)
            mine(1, burn)
            sequence, witness = describe_spend(txid)
            check("B. Liana spends Savings with no beneficiary with 2 of 3 keys (PHONE + HELPER)",
                  mined_in_tip(txid) and witness == 5, f"txid {txid}")
        else:
            psbt = core_psbt("plain-watch", plain_outpoint)
            for holder in ("phone", "device"):
                psbt = sign(psbt, plain_signers[holder])
            txid = cli("sendrawtransaction", core_finalize(psbt))
            mine(1, burn)
            sequence, witness = describe_spend(txid)
            check("B. Bitcoin Core alone spends Savings with no beneficiary with 2 of 3 keys (PHONE + DEVICE)",
                  mined_in_tip(txid) and witness == 5 and sequence >= 0xFFFFFFFD, f"txid {txid}")
    finally:
        stop(lianad)
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
