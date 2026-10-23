# Security policy

## Reporting a vulnerability

Please report vulnerabilities **privately**, through GitHub's **Report a vulnerability** button on this repository's [Security tab](https://github.com/brent472/twofold/security/advisories/new).

**Never open a public issue, pull request or discussion about a vulnerability.** That includes "I think I found something" messages, which can point others at it before it's fixed.

Helpful things to include:

- what's affected (a file, function, command or spending path)
- how to reproduce it, ideally on regtest
- what you think the impact is

Please don't include real private keys, recovery phrases or wallet files in a report. Test keys made for the report are fine.

## Scope: test networks only, for now

Twofold is pre-alpha. It runs on **regtest and signet only**, refuses mainnet, and holds no real bitcoin. Reports about the Savings spending rules, key handling, the check-in, the exit drill or the build and release process are all welcome. There are no real funds at risk yet.

## No bug bounty yet

There's no bug bounty programme at the moment. Reports are still very welcome.
