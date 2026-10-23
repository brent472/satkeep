use crate::Role;

/// Everything that can go wrong when setting up a Savings wallet.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the device delay must be at least 1 block")]
    DeviceDelayZero,
    #[error("the heir delay ({heir} blocks) must be longer than the device delay ({device} blocks)")]
    HeirDelayNotLonger { device: u16, heir: u16 },
    #[error("the {0} key must be an extended public key with receive and change paths (…/<0;1>/*)")]
    KeyShape(Role),
    #[error("the {0} key is a mainnet key; Twofold only runs on test networks for now")]
    MainnetKey(Role),
    #[error("the {0} and {1} keys are the same; every key in Savings must be different")]
    DuplicateKey(Role, Role),
    #[error("Twofold only runs on test networks for now; mainnet is refused")]
    Mainnet,
    #[error("input {input} needs {needed} confirmations before it can be spent this way, and has {has}")]
    TooEarly { input: usize, needed: u16, has: u32 },
    #[error("input {input} uses a time-based delay; Savings only uses block-based delays")]
    TimeBasedDelay { input: usize },
    #[error("input {input} spends a coin this wallet doesn't know")]
    UnknownCoin { input: usize },
    #[error("not enough keys have approved this payment")]
    NotEnoughApprovals,
    #[error("that isn't a valid test-network private key for a Savings key holder")]
    BadHolderKey,
    #[error("BDK could not build the payment: {0}")]
    Build(#[from] bdk_wallet::error::CreateTxError),
    #[error("could not finalize the payment: {0}")]
    CannotFinalize(String),
    #[error("BDK could not sign or finalize the payment: {0}")]
    Finalize(#[from] bdk_wallet::signer::SignerError),
    #[error("could not extract the finished transaction: {0}")]
    Extract(#[from] crate::bitcoin::psbt::ExtractTxError),
    #[error("the check-in interval must be at least 1 block")]
    CheckinIntervalZero,
    #[error("the check-in margin must be at least 1 block")]
    CheckinMarginZero,
    #[error("the check-in interval ({interval} blocks) must be shorter than the device delay ({device} blocks) by at least the margin ({margin} blocks)")]
    CheckinTooLate { interval: u16, margin: u16, device: u16 },
    #[error("there are no Savings coins to check in")]
    CheckinNoCoins,
    #[error("this payment isn't a check-in")]
    NotACheckin,
    #[error("a check-in must use the 2-of-3 path, not a delayed path")]
    CheckinUsesDelayedPath,
    #[error("a check-in must send everything to one new Savings address, and nothing anywhere else")]
    CheckinPaysOut,
    #[error("this check-in leaves out coin {coin}; every coin must be included (run `twofold checkin` again)")]
    CheckinMissesCoin { coin: crate::bitcoin::OutPoint },
    #[error("miniscript rejected the Savings descriptor: {0}")]
    Miniscript(#[from] crate::miniscript::Error),
}
