//! The Twofold Savings wallet policy.
//!
//! Savings can be spent by any 2 of PHONE, DEVICE and HELPER. After a delay
//! DEVICE can spend alone, and after a longer delay HEIR can spend alone.
//! See the crate README for every assumption behind this policy.

pub use bdk_wallet::{bitcoin, miniscript};
