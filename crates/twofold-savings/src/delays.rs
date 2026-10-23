use crate::Error;

/// Placeholder heir delay: about 14 months of blocks. Not final until an
/// outside Bitcoin reviewer signs off.
pub const DEFAULT_HEIR_DELAY: u16 = 61_320;

/// The Savings wallet's delayed path, if it has one.
///
/// Savings starts with no beneficiary: plain 2 of 3, with no delayed path.
/// Once a beneficiary is added, HEIR alone can spend a coin that has waited
/// the heir delay. There is no other delayed path (the card-only path was
/// removed; the phone key backup covers a lost phone).
///
/// Delays are relative (`older`, BIP 68/112) and counted in blocks only. The
/// `u16` type caps them at 65,535 blocks, the consensus limit for a
/// block-based relative timelock, and makes time-based delays impossible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delays {
    heir: Option<u16>,
}

impl Delays {
    /// No delayed path: Savings with no beneficiary yet.
    pub const fn none() -> Self {
        Self { heir: None }
    }

    /// HEIR alone can spend once a coin has waited `heir_blocks`, at least 1.
    pub fn with_heir(heir_blocks: u16) -> Result<Self, Error> {
        if heir_blocks == 0 {
            return Err(Error::HeirDelayZero);
        }
        Ok(Self { heir: Some(heir_blocks) })
    }

    /// Blocks before HEIR can spend alone, or `None` with no beneficiary.
    pub fn heir(&self) -> Option<u16> {
        self.heir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_heir_delay_is_the_placeholder() {
        assert_eq!(Delays::with_heir(DEFAULT_HEIR_DELAY).unwrap().heir(), Some(61_320));
    }

    #[test]
    fn no_beneficiary_means_no_delay() {
        assert_eq!(Delays::none().heir(), None);
    }

    #[test]
    fn accepts_the_relative_timelock_cap() {
        assert_eq!(Delays::with_heir(u16::MAX).unwrap().heir(), Some(65_535));
    }

    #[test]
    fn rejects_a_zero_heir_delay() {
        assert!(matches!(Delays::with_heir(0), Err(Error::HeirDelayZero)));
    }
}
