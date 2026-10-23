use crate::Error;

/// Placeholder device delay: about 9 months of blocks. Not final until an
/// outside Bitcoin reviewer signs off.
pub const DEFAULT_DEVICE_DELAY: u16 = 39_420;

/// Placeholder heir delay: about 14 months of blocks. Not final until an
/// outside Bitcoin reviewer signs off.
pub const DEFAULT_HEIR_DELAY: u16 = 61_320;

/// How many blocks a coin must wait, after it confirms, before the delayed
/// paths open.
///
/// Delays are relative (`older`, BIP 68/112) and counted in blocks only. The
/// `u16` type caps them at 65,535 blocks, the consensus limit for a
/// block-based relative timelock, and makes time-based delays impossible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delays {
    device: u16,
    heir: u16,
}

impl Delays {
    /// Checks that the device delay is at least one block and the heir delay
    /// is longer than the device delay.
    pub fn new(device_blocks: u16, heir_blocks: u16) -> Result<Self, Error> {
        if device_blocks == 0 {
            return Err(Error::DeviceDelayZero);
        }
        if heir_blocks <= device_blocks {
            return Err(Error::HeirDelayNotLonger { device: device_blocks, heir: heir_blocks });
        }
        Ok(Self { device: device_blocks, heir: heir_blocks })
    }

    /// Blocks before DEVICE can spend alone.
    pub fn device(&self) -> u16 {
        self.device
    }

    /// Blocks before HEIR can spend alone.
    pub fn heir(&self) -> u16 {
        self.heir
    }
}

impl Default for Delays {
    fn default() -> Self {
        Self { device: DEFAULT_DEVICE_DELAY, heir: DEFAULT_HEIR_DELAY }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_placeholder_delays() {
        let d = Delays::default();
        assert_eq!((d.device(), d.heir()), (39_420, 61_320));
        assert_eq!(Delays::new(39_420, 61_320).unwrap(), d);
    }

    #[test]
    fn accepts_the_relative_timelock_cap() {
        let d = Delays::new(1, u16::MAX).unwrap();
        assert_eq!(d.heir(), 65_535);
    }

    #[test]
    fn rejects_a_zero_device_delay() {
        assert!(matches!(Delays::new(0, 10), Err(Error::DeviceDelayZero)));
    }

    #[test]
    fn rejects_an_heir_delay_that_is_not_longer() {
        assert!(matches!(Delays::new(10, 10), Err(Error::HeirDelayNotLonger { .. })));
        assert!(matches!(Delays::new(20, 10), Err(Error::HeirDelayNotLonger { .. })));
    }
}
