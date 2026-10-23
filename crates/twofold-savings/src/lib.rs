//! The Twofold Savings wallet policy.
//!
//! Savings can be spent by any 2 of PHONE, DEVICE and HELPER. Once a
//! beneficiary is added, HEIR can also spend alone after the heir delay
//! ([`migrate`] moves Savings to that wallet). See the crate README for
//! every assumption behind this policy.
//!
//! [`backup`] is the phone key backup prototype: the phone key encrypted to
//! the owner's cloud, unlocked only with their security device.

pub mod backup;
pub mod checkin;
mod delays;
mod error;
mod holder;
mod keys;
pub mod migrate;
mod policy;
pub mod spend;

pub use bdk_wallet::{bitcoin, miniscript};
pub use delays::{DEFAULT_HEIR_DELAY, Delays};
pub use error::Error;
pub use holder::{HolderKey, PHRASE_WORDS, contains_recovery_phrase};
pub use keys::{Role, SavingsKeys};
pub use policy::{NUMS_POINT, SavingsPolicy, unspendable_internal_key};
