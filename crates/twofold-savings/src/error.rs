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
    #[error("miniscript rejected the Savings descriptor: {0}")]
    Miniscript(#[from] crate::miniscript::Error),
}
