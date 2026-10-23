//! The Twofold Savings wallet policy.
//!
//! Savings can be spent by any 2 of PHONE, DEVICE and HELPER. After a delay
//! DEVICE can spend alone, and after a longer delay HEIR can spend alone.
//! See the crate README for every assumption behind this policy.

pub mod checkin;
mod delays;
mod error;
mod holder;
mod keys;
mod policy;
pub mod spend;

pub use bdk_wallet::{bitcoin, miniscript};
pub use delays::{DEFAULT_DEVICE_DELAY, DEFAULT_HEIR_DELAY, Delays};
pub use error::Error;
pub use holder::HolderKey;
pub use keys::{Role, SavingsKeys};
pub use policy::{NUMS_POINT, SavingsPolicy, unspendable_internal_key};
