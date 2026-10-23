use crate::Role;

/// Everything that can go wrong when setting up a Savings wallet.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the heir delay must be at least 1 block")]
    HeirDelayZero,
    #[error("a HEIR key needs an heir delay, and an heir delay needs a HEIR key")]
    HeirAndDelayMismatch,
    #[error("this Savings has no beneficiary, so there is no heir path")]
    NoHeirPath,
    #[error("the {0} key must be an extended public key with receive and change paths (…/<0;1>/*)")]
    KeyShape(Role),
    #[error("the {0} key is a mainnet key; Twofold only runs on test networks for now")]
    MainnetKey(Role),
    #[error("the {0} and {1} keys are the same; every key in Savings must be different")]
    DuplicateKey(Role, Role),
    #[error("Twofold only runs on test networks for now; mainnet is refused")]
    Mainnet,
    #[error("couldn't build the unspendable internal key from these keys")]
    InternalKey,
    #[error("input {input} needs {needed} confirmations before it can be spent this way, and has {has}")]
    TooEarly { input: usize, needed: u16, has: u32 },
    #[error("input {input} uses a time-based delay; Savings only uses block-based delays")]
    TimeBasedDelay { input: usize },
    #[error("input {input} spends a coin this wallet doesn't know")]
    UnknownCoin { input: usize },
    #[error("input {input}'s amount or script in this payment doesn't match the wallet's own record of that coin; the payment may have been altered")]
    InputMismatch { input: usize },
    #[error("input {input} spends a P2WSH coin but this payment doesn't include the coin's previous transaction, which signers need to check its amount")]
    MissingPreviousTransaction { input: usize },
    #[error("not enough keys have approved this payment")]
    NotEnoughApprovals,
    #[error("that isn't a valid key for a Savings key holder: it must be a 12-word recovery phrase")]
    BadHolderKey,
    #[error("that key is in the old tprv format; Twofold keys are now recovery phrases, so make a new key with keygen")]
    OldHolderKey,
    #[error("BDK could not build the payment: {0}")]
    Build(#[source] Box<bdk_wallet::error::CreateTxError>),
    #[error("could not finalize the payment: {0}")]
    CannotFinalize(String),
    #[error("BDK could not sign or finalize the payment: {0}")]
    Finalize(#[source] Box<bdk_wallet::signer::SignerError>),
    #[error("could not extract the finished transaction: {0}")]
    Extract(#[source] Box<crate::bitcoin::psbt::ExtractTxError>),
    #[error("the check-in interval must be at least 1 block")]
    CheckinIntervalZero,
    #[error("the check-in margin must be at least 1 block")]
    CheckinMarginZero,
    #[error("the check-in interval ({interval} blocks) must be shorter than the heir delay ({heir} blocks) by at least the margin ({margin} blocks)")]
    CheckinTooLate { interval: u16, margin: u16, heir: u16 },
    #[error("this Savings has no beneficiary, so it needs no check-in")]
    NoCheckinWithoutHeir,
    #[error("there are no confirmed Savings coins to check in")]
    CheckinNoCoins,
    #[error("a check-in must not spend unconfirmed coin {coin}; its delays haven't started yet")]
    CheckinSpendsUnconfirmed { coin: crate::bitcoin::OutPoint },
    #[error("this payment isn't a check-in")]
    NotACheckin,
    #[error("a check-in must use the 2-of-3 path, not a delayed path")]
    CheckinUsesDelayedPath,
    #[error("a check-in must send everything to one new Savings address, and nothing anywhere else")]
    CheckinPaysOut,
    #[error("this check-in leaves out coin {coin}; every coin must be included (run `twofold checkin` again)")]
    CheckinMissesCoin { coin: crate::bitcoin::OutPoint },
    #[error("this payment isn't a move to a new Savings wallet")]
    NotAMove,
    #[error("a move must go to a Savings wallet with a beneficiary and the same PHONE, DEVICE and HELPER keys")]
    MoveToWrongWallet,
    #[error(
        "this move names a different beneficiary or heir delay than the one set up in this wallet's settings;          nothing goes anywhere this wallet's settings don't say"
    )]
    MoveNotAsSetUp,
    #[error("the heir delay is {heir} blocks, below the minimum of {minimum} blocks")]
    HeirDelayTooShort { heir: u16, minimum: u16 },
    #[error("there are no confirmed Savings coins to move")]
    MoveNoCoins,
    #[error("a move must not spend unconfirmed coin {coin}")]
    MoveSpendsUnconfirmed { coin: crate::bitcoin::OutPoint },
    #[error("this move leaves out coin {coin}; every confirmed coin must be included (prepare it again)")]
    MoveMissesCoin { coin: crate::bitcoin::OutPoint },
    #[error("a move must use the 2-of-3 path and send everything to the new wallet's first address")]
    MoveShape,
    #[error("miniscript rejected the Savings descriptor: {0}")]
    Miniscript(#[source] Box<crate::miniscript::Error>),
}

// Other crates' errors are boxed so `Error`, and every `Result` that carries
// it, stays small.
macro_rules! boxed_from {
    ($($source:ty => $variant:ident),* $(,)?) => {$(
        impl From<$source> for Error {
            fn from(e: $source) -> Self {
                Error::$variant(Box::new(e))
            }
        }
    )*};
}

boxed_from! {
    bdk_wallet::error::CreateTxError => Build,
    bdk_wallet::signer::SignerError => Finalize,
    crate::bitcoin::psbt::ExtractTxError => Extract,
    crate::miniscript::Error => Miniscript,
}
