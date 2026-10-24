# Signet demo: one transaction for each allowed Savings path, in both shapes,
# on the public signet test network. See docs/signet-demo.md.
#
# Run one step at a time, from the repo root, with a signet node running
# (bitcoind -signet) and synced:
#
#   .\contrib\signet-demo\demo.ps1 setup     # keys, plain Savings, a funding address
#   .\contrib\signet-demo\demo.ps1 plain     # 2 of 3, once per pair of keys
#   .\contrib\signet-demo\demo.ps1 move      # add a beneficiary: one 2-of-3 move
#   .\contrib\signet-demo\demo.ps1 taproot   # 2 of 3 with the heir path in place, and a check-in
#   .\contrib\signet-demo\demo.ps1 heir      # HEIR alone, once the heir delay has passed
#
# Test networks only. The four keys are fresh test keys, saved under
# $env:SATKEEP_HOME (default %USERPROFILE%\.satkeep) as signet\keys\*.key,
# outside the repo. Nothing here prints a private key or recovery phrase:
# keygen prints only public keys, and sign reads each key from its file.
# Each step waits for its transactions to confirm (about 10 minutes a block).

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("setup", "plain", "move", "taproot", "heir")]
    [string]$Step
)

# Native commands report failure through exit codes, checked below.
$ErrorActionPreference = "Continue"

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$tf = Join-Path $repo "target\debug\satkeep.exe"
$bcli = if ($env:BITCOIN_CLI) { $env:BITCOIN_CLI } else { "C:\Program Files\Bitcoin\daemon\bitcoin-cli.exe" }
$base = if ($env:SATKEEP_HOME) { $env:SATKEEP_HOME } else { Join-Path $env:USERPROFILE ".satkeep" }
$plain = Join-Path $base "signet"          # Savings with no beneficiary
$heirs = Join-Path $base "signet-heir"     # Savings with a beneficiary
$keys = Join-Path $plain "keys"
$publicKeys = Join-Path $base "signet-demo-public-keys.txt"
$log = Join-Path $base "signet-demo-txids.txt"

# Short test delays, so the heir path opens in about two hours of signet
# blocks instead of the placeholder 61,320. The minimum heir delay is
# lowered to match.
$heirDelay = 12
$checkinInterval = 6
$checkinMargin = 3
$refreshWindow = 2
$minHeirDelay = 10
$payment = "0.00001"   # 1,000 sats per payment

function Fail([string]$message) {
    Write-Host "demo: $message" -ForegroundColor Red
    exit 1
}

# Runs satkeep on signet; returns its output, or stops with its error.
function Satkeep([string]$wallet, [string[]]$arguments, [switch]$AllowFailure) {
    # 2>&1 turns each stderr line into an error record; keep its plain text.
    $lines = & $tf --network signet --wallet-dir $wallet @arguments 2>&1
    $code = $LASTEXITCODE
    $out = @($lines | Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] } | ForEach-Object { "$_" })
    $err = @($lines | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] } | ForEach-Object { $_.Exception.Message }) -join "`n"
    if ($code -ne 0) {
        if ($AllowFailure) { return @{ ok = $false; out = $out; err = $err } }
        Fail "satkeep $($arguments[0]) failed: $err"
    }
    if ($AllowFailure) { return @{ ok = $true; out = $out; err = $err } }
    $out
}

function Node([string[]]$arguments) {
    $out = & $bcli -signet @arguments
    if ($LASTEXITCODE -ne 0) { Fail "bitcoin-cli $($arguments[0]) failed" }
    $out
}

# Signs with two holders, broadcasts, logs the transaction ID, and waits for
# it to confirm.
function Approve-And-Send([string]$wallet, [string]$psbt, [string[]]$holders, [string]$label) {
    foreach ($holder in $holders) {
        Satkeep $wallet @("sign", "--psbt", $psbt, "--yes", "--key-file", (Join-Path $keys "$holder.key")) | Out-Null
    }
    $sent = Satkeep $wallet @("broadcast", "--psbt", $psbt)
    $txid = ($sent | Select-String -Pattern "Transaction ID: ([0-9a-f]{64})").Matches[0].Groups[1].Value
    if (-not $txid) { Fail "no transaction ID in: $sent" }
    Add-Content -Path $log -Value "$label`t$txid"
    Write-Host "$label`t$txid"
    Wait-Confirmed $txid
    $txid
}

function Wait-Confirmed([string]$txid) {
    while ((Node @("getrawmempool")) -match $txid) {
        Start-Sleep -Seconds 60
    }
    Write-Host "  confirmed at block $(Node @('getblockcount'))"
}

# An address nobody can spend from (the NUMS point), for the payments.
function Burn-Address {
    $nums = (Node @("getdescriptorinfo", "rawtr(50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0)") | ConvertFrom-Json).descriptor
    (Node @("deriveaddresses", $nums) | ConvertFrom-Json)[0]
}

function Pay([string]$wallet, [string]$name, [string[]]$holders, [string]$label) {
    $psbt = Join-Path $wallet "$name.psbt"
    Satkeep $wallet @("send", "--to", (Burn-Address), "--amount", $payment, "--out", $psbt) | Write-Host
    Approve-And-Send $wallet $psbt $holders $label | Out-Null
}

$info = Node @("getblockchaininfo") | ConvertFrom-Json
if ($info.chain -ne "signet") { Fail "the node isn't on signet" }
if ($info.initialblockdownload) { Fail "the signet node is still syncing (block $($info.blocks) of $($info.headers))" }

switch ($Step) {
    "setup" {
        cargo build -p satkeep-cli
        if ($LASTEXITCODE -ne 0) { Fail "build failed" }
        if (Test-Path $publicKeys) { Fail "$publicKeys already exists: this demo was set up before" }
        $pub = [ordered]@{}
        foreach ($name in "phone", "device", "helper", "heir") {
            $out = & $tf --network signet keygen --name $name
            if ($LASTEXITCODE -ne 0) { Fail "keygen failed" }
            $pub[$name] = ($out | Where-Object { $_ -like "  *" }).Trim()
        }
        $pub.GetEnumerator() | ForEach-Object { "$($_.Key)`t$($_.Value)" } | Set-Content $publicKeys
        Satkeep $plain @("create", "--phone", $pub.phone, "--device", $pub.device, "--helper", $pub.helper,
            "--min-heir-delay", "$minHeirDelay") | Write-Host
        $address = (Satkeep $plain @("address")).Trim()
        Write-Host ""
        Write-Host "Fund Savings with about 0.0005 signet BTC (50,000 sats) at:"
        Write-Host "  $address"
        Write-Host "then wait for one confirmation and run: .\contrib\signet-demo\demo.ps1 plain"
    }
    "plain" {
        Satkeep $plain @("balance") | Write-Host
        Pay $plain "pay-phone-device" @("phone", "device") "plain 2-of-3 (PHONE + DEVICE)"
        Pay $plain "pay-phone-helper" @("phone", "helper") "plain 2-of-3 (PHONE + HELPER)"
        Pay $plain "pay-device-helper" @("device", "helper") "plain 2-of-3 (DEVICE + HELPER)"
        Satkeep $plain @("balance") | Write-Host
    }
    "move" {
        $heir = ((Get-Content $publicKeys | Where-Object { $_ -like "heir`t*" }) -split "`t")[1]
        $psbt = Join-Path $plain "move.psbt"
        Satkeep $plain @("add-heir", "--heir", $heir, "--heir-delay", "$heirDelay",
            "--checkin-interval", "$checkinInterval", "--checkin-margin", "$checkinMargin",
            "--refresh-window", "$refreshWindow", "--new-wallet-dir", $heirs, "--out", $psbt) | Write-Host
        Approve-And-Send $plain $psbt @("phone", "device") "move to Taproot (PHONE + DEVICE)" | Out-Null
        Satkeep $heirs @("balance") | Write-Host
    }
    "taproot" {
        Pay $heirs "pay-device-helper" @("device", "helper") "Taproot 2-of-3 (DEVICE + HELPER)"
        Pay $heirs "pay-phone-device" @("phone", "device") "Taproot 2-of-3 (PHONE + DEVICE)"
        $psbt = Join-Path $heirs "checkin.psbt"
        Satkeep $heirs @("checkin", "--out", $psbt) | Write-Host
        Approve-And-Send $heirs $psbt @("phone", "helper") "Taproot check-in (PHONE + HELPER)" | Out-Null
        Satkeep $heirs @("balance") | Write-Host
    }
    "heir" {
        $psbt = Join-Path $heirs "heir.psbt"
        $refusedOnce = $false
        while ($true) {
            $try = Satkeep $heirs @("send", "--to", (Burn-Address), "--all", "--path", "heir", "--out", $psbt) -AllowFailure
            if ($try.ok) { $try.out | Write-Host; break }
            if ($try.err -notmatch "needs $heirDelay confirmations") { Fail "unexpected refusal: $($try.err)" }
            if (-not $refusedOnce) {
                Add-Content -Path $log -Value "HEIR alone refused before the heir delay`t$($try.err.Trim())"
                $refusedOnce = $true
            }
            Write-Host "  $($try.err.Trim()); waiting for the next block"
            Start-Sleep -Seconds 300
        }
        Approve-And-Send $heirs $psbt @("heir") "Taproot HEIR alone, after $heirDelay blocks" | Out-Null
        Satkeep $heirs @("balance") | Write-Host
    }
}
