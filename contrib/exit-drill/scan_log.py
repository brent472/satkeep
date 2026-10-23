#!/usr/bin/env python3
"""Fails if a log holds anything that looks like key material.

    scan_log.py LOG --words ENGLISH_RS [--keys DIR]

Checks, in order:
  - `tprv` or `xprv` anywhere
  - 12 BIP39 English words in a row (a recovery phrase), ignoring case and
    anything between words that isn't a letter
  - with --keys: 3 words in a row from any recovery phrase in DIR/*.key

ENGLISH_RS is the bip39 crate's `src/language/english.rs`; the 2,048 words
are read from its string literals. Only which check failed is printed, never
the text that matched. Exit code 1 means the log failed a check.
"""

import argparse
import glob
import os
import re
import sys

PHRASE_WORDS = 12
KNOWN_RUN = 3


def words_of(text):
    return [w.lower() for w in re.split(r"[^A-Za-z]+", text) if w]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("log")
    parser.add_argument("--words", required=True)
    parser.add_argument("--keys")
    args = parser.parse_args()

    with open(args.words, encoding="utf-8") as f:
        wordlist = set(re.findall(r'"([a-z]+)"', f.read()))
    if len(wordlist) != 2048:
        print(f"scan_log: expected 2048 BIP39 words in {args.words}, found {len(wordlist)}")
        return 1

    with open(args.log, encoding="utf-8", errors="replace") as f:
        log = f.read()
    failed = []

    if "tprv" in log or "xprv" in log:
        failed.append("the log contains an extended private key prefix")

    printed = words_of(log)
    run = longest = 0
    for word in printed:
        run = run + 1 if word in wordlist else 0
        longest = max(longest, run)
    if longest >= PHRASE_WORDS:
        failed.append(f"the log has {longest} BIP39 words in a row (a recovery phrase is {PHRASE_WORDS})")

    phrases = 0
    if args.keys:
        runs = {tuple(printed[i:i + KNOWN_RUN]) for i in range(len(printed) - KNOWN_RUN + 1)}
        for path in sorted(glob.glob(os.path.join(args.keys, "*.key"))):
            with open(path, encoding="utf-8") as f:
                phrase = words_of(f.read())
            phrases += 1
            if any(tuple(phrase[i:i + KNOWN_RUN]) in runs for i in range(len(phrase) - KNOWN_RUN + 1)):
                failed.append(f"the log has {KNOWN_RUN} words in a row from the phrase in {os.path.basename(path)}")
        if phrases == 0:
            failed.append(f"no key files found in {args.keys} to check against")

    for reason in failed:
        print(f"::error::Key material check failed: {reason}")
    if failed:
        return 1
    extra = f", and nothing from the {phrases} known phrases" if args.keys else ""
    print(f"Key material check passed: no extended private key prefix, at most {longest} BIP39 words in a row{extra}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
