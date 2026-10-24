# Exit guide: getting Savings out without Satkeep (DRAFT)

> **Draft for review. This isn't app copy.** The words people see in the app come from Section 19 of the brief. This page is for a family member, a friend or a helper sitting at a computer with the owner, or for the heir.
>
> **Test networks only, for now.** Everything here works on the test networks (regtest and signet), and the restore tool refuses real bitcoin until the launch review. Some steps are marked *to be decided*: they depend on how releases will be published.

Satkeep keeps Savings so that it never depends on Satkeep. If the app is gone, or the company is, you can still move the bitcoin with free, open-source apps. This page walks through it.

## What you need

- **The exit kit.** It's a PDF in the owner's cloud drive (iCloud Drive or Google Drive), in a folder named for Satkeep. A printed copy works too.
- **The second key** and its PIN. That's the security device that isn't the phone: for example a Trezor, or the owner's old phone or tablet with the Satkeep app still on it.
- **A computer** you trust. If you can, use one that isn't used for much else.
- **The restore tool, `satkeep-restore`.** It's free and open source. *To be decided:* where to download it, and the signing key to check it against. The kit will say.

**If you're the heir,** you need your own 12 words instead of the second key. See [If you're the heir](#if-youre-the-heir).

## Step 1: Get the apps, from their official sites only

The restore tool tells you which app each part of Savings needs:

- **Savings with a beneficiary** goes into **Liana**: [wizardsardine.com/liana](https://wizardsardine.com/liana).
- **Savings with no beneficiary** goes into **Sparrow**: [sparrowwallet.com](https://sparrowwallet.com).

**Download only from those sites,** and check the download's signature as each site explains, before you open it. A fake app is the easiest way to lose everything.
- **Liana:** its downloads are signed by the key with fingerprint `5B63 F3B9 7699 C7EE F3B0 40B1 9B7F 629A 53E7 7B83`. That's the one our own test checks against.
- **Sparrow:** follow the verification steps on Sparrow's own download page.

## Step 2: See what's in the kit

Run the restore tool on the kit:

```
satkeep-restore show --kit "Satkeep exit kit.pdf"
```

It shows:
- the date the backup was made;
- which second key can unlock it;
- the wallets in it.

Nothing is unlocked yet, and nothing secret is shown. The list of wallets says **UNVERIFIED** until the second key unlocks the kit. Anyone with access to the cloud drive could have changed that copy.

## Step 3: Unlock it with the second key, and follow the tool

Connect the second key, then run the command for the app you're using. Enter the PIN on the second key itself when it asks.

```
satkeep-restore liana --kit "Satkeep exit kit.pdf"
satkeep-restore sparrow --kit "Satkeep exit kit.pdf" --out savings-for-sparrow.txt
```

**What the tool checks:**
- that the kit hasn't been changed;
- that the phone's key really belongs to these wallets.

If anything doesn't match, it stops. Don't work around that.

**What it prints:** which app to use for each wallet, and one next step at the end. Follow that step.

**If the second key is the owner's old phone or tablet,** there's nothing to connect. The phone needs the Satkeep app still on it, but no internet:
1. The tool shows an **unlock request**: a block of letters and numbers between two lines that say SATKEEP UNLOCK REQUEST.
2. Give it to the Satkeep app on that phone. *To be decided:* how, scanning it or typing it, comes with the app.
3. The phone checks the request is for it, and asks for its own face, fingerprint or passcode.
4. It shows an **answer** of 72 letters and numbers. Type it into the tool; spaces don't matter.
- **A typo** is caught, and you can type the answer again.
- **Keep both to yourself:** the request and the answer together unlock the backup. Close the window afterwards.

*To be decided:* this version of the tool talks to the old-phone second key and to a test one. Trezor comes in a later version.

## Step 4a: In Liana (Savings with a beneficiary)

1. **Create the wallet:** start Liana and add a wallet from an existing description, pasting the line the tool printed after "Liana's main descriptor". *To be checked against Liana's screens.*
2. **Add the phone's key.** Run the tool again with `--show-phrase` to see the phone key's 12 words, and add them in Liana as a "hot signer".
   - **Keep them safe:** anyone who sees those 12 words holds one of the keys.
   - **Do it in private,** and close the window afterwards.
3. **Add the second key** in Liana as a signing device, if Liana supports it.
4. **Send:** make a payment to your own new address, approve it with both keys, and send it.

## Step 4b: In Sparrow (Savings with no beneficiary)

1. **Import the wallet:** start Sparrow, choose to import a wallet from an output descriptor, and pick the file the tool wrote. *To be checked against Sparrow's screens.*
2. **Add the phone's key:** run the tool again with `--show-phrase` and add the 12 words to Sparrow as that key. The same care applies as in Liana.
3. **Add the second key** in Sparrow.
4. **Send:** make a payment, approve it with both keys, and send it.

## If you're the heir

You have your own 12 words, written down or kept by you, and the kit. You don't need the second key.

```
satkeep-restore heir --kit "Satkeep exit kit.pdf" --key-file my-words.txt
```

**Keep your words off the command line.** Put them in a file, or type them when asked with `--key-stdin`. Never put them in the command itself.

**What the tool tells you:**
- which wallets you can claim;
- how long you must wait: a number of blocks, with a rough time, about 14 months as Satkeep is set up today.

**You can only move coins that haven't moved for that long.** Every check-in or payment by the owner starts a coin's wait again.

**Its output says UNVERIFIED:** without the second key nobody can check the kit hasn't been changed. Your key can only ever spend from wallets that contain it, so a changed kit can't make you send someone else's coins. It can still mislead you about what's there.

**Then, in Liana:**
1. add the wallet from the line the tool printed;
2. add your 12 words as a hot signer;
3. once the wait is over, use Liana's recovery to send the coins to **a separate wallet that only you control**. Never send them to an address from the wallet you just added: because the kit is unverified, that wallet could include someone else's key.

## For experts: Bitcoin Core

**Bitcoin Core works for every Savings wallet,** with no other app.

```
satkeep-restore core --kit "Satkeep exit kit.pdf"
```

It imports each wallet into a new, encrypted wallet in your own Bitcoin Core, on the same computer. You choose its passphrase, and the node rescans from when each wallet was made.

**To spend:**
1. unlock the wallet with its passphrase;
2. have Core approve as the phone;
3. add the second key's approval;
4. send.

**Or, with `--out FILE`,** it writes Core's import file instead. That file holds the phone key unencrypted, so delete it once it's imported. [`docs/exit-drill.md`](exit-drill.md) shows every step of this done by hand.

## When something goes wrong

- **"that key can't unlock this backup":** this second key isn't the one the backup is locked to. Try the owner's other device.
- **"wrong PIN":** the second key counts wrong PINs and locks itself after too many. Stop and find the right PIN.
- **"that answer has a typo, or it answers a different unlock request":** check the answer on the phone and type it again. An answer only works for the request it was made for. If you started the tool again, give the phone the new request.
- **"this backup isn't locked to this phone"** (on the phone): it's the wrong phone or tablet. Try the owner's other device.
- **"the kit has been changed":** the plain copy in the kit doesn't match the locked one. Don't use it. Look for another copy of the kit, such as a printout or an older download.
- **"the phone key … isn't in Savings wallet":** the backup and the wallet list don't belong together. Don't use this kit.
