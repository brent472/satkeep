use std::fmt;

use crate::Error;
use crate::bitcoin::NetworkKind;
use crate::miniscript::descriptor::{DescriptorPublicKey, Wildcard};

/// Who holds a key in the Savings policy.
///
/// DEVICE holds two keys: [`Role::Device`] for the everyday 2-of-3 path and
/// [`Role::DeviceDelayed`] for the delayed device-only path. Keeping them
/// separate is what Liana expects and what miniscript requires for SegWit v0;
/// it does not change who can spend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Phone,
    Device,
    DeviceDelayed,
    Helper,
    Heir,
}

impl Role {
    pub const ALL: [Role; 5] = [Role::Phone, Role::Device, Role::DeviceDelayed, Role::Helper, Role::Heir];
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Phone => "PHONE",
            Role::Device => "DEVICE",
            Role::DeviceDelayed => "DEVICE_R",
            Role::Helper => "HELPER",
            Role::Heir => "HEIR",
        })
    }
}

/// The five public keys that make up a Savings wallet.
///
/// Each key must be a test-network extended public key with a receive and a
/// change path (`…/<0;1>/*`), and no two keys may share an extended key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavingsKeys {
    phone: DescriptorPublicKey,
    device: DescriptorPublicKey,
    device_delayed: DescriptorPublicKey,
    helper: DescriptorPublicKey,
    heir: DescriptorPublicKey,
}

impl SavingsKeys {
    pub fn new(
        phone: DescriptorPublicKey,
        device: DescriptorPublicKey,
        device_delayed: DescriptorPublicKey,
        helper: DescriptorPublicKey,
        heir: DescriptorPublicKey,
    ) -> Result<Self, Error> {
        let keys = Self { phone, device, device_delayed, helper, heir };
        for role in Role::ALL {
            check_shape(role, keys.get(role))?;
        }
        for (i, a) in Role::ALL.iter().enumerate() {
            for b in &Role::ALL[i + 1..] {
                if xpub(keys.get(*a)) == xpub(keys.get(*b)) {
                    return Err(Error::DuplicateKey(*a, *b));
                }
            }
        }
        Ok(keys)
    }

    pub fn get(&self, role: Role) -> &DescriptorPublicKey {
        match role {
            Role::Phone => &self.phone,
            Role::Device => &self.device,
            Role::DeviceDelayed => &self.device_delayed,
            Role::Helper => &self.helper,
            Role::Heir => &self.heir,
        }
    }
}

fn check_shape(role: Role, key: &DescriptorPublicKey) -> Result<(), Error> {
    let DescriptorPublicKey::MultiXPub(k) = key else {
        return Err(Error::KeyShape(role));
    };
    // Two different branches. `<1;1>` parses as two but collapses to one
    // when written back out, and would make receive and change the same.
    let paths = k.derivation_paths.paths();
    let receive_and_change = paths.len() == 2 && paths[0] != paths[1];
    if !receive_and_change || k.wildcard != Wildcard::Unhardened {
        return Err(Error::KeyShape(role));
    }
    if k.xkey.network == NetworkKind::Main {
        return Err(Error::MainnetKey(role));
    }
    Ok(())
}

fn xpub(key: &DescriptorPublicKey) -> Option<crate::bitcoin::bip32::Xpub> {
    match key {
        DescriptorPublicKey::MultiXPub(k) => Some(k.xkey),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod test_keys {
    use std::str::FromStr;

    use rand::RngCore;

    use crate::bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
    use crate::bitcoin::secp256k1::Secp256k1;
    use crate::bitcoin::{Network, NetworkKind};
    use crate::miniscript::descriptor::DescriptorPublicKey;

    /// A fresh random key as `[fp/87'/1'/0']tpub…/<0;1>/*`. The private half is
    /// dropped immediately.
    pub fn fresh(network: Network) -> DescriptorPublicKey {
        let secp = Secp256k1::new();
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let master = Xpriv::new_master(NetworkKind::from(network), &seed).unwrap();
        let path = DerivationPath::from_str("m/87'/1'/0'").unwrap();
        let account = Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap());
        let fp = master.fingerprint(&secp);
        DescriptorPublicKey::from_str(&format!("[{fp}/87'/1'/0']{account}/<0;1>/*")).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::test_keys::fresh;
    use super::*;
    use crate::bitcoin::Network;

    fn five() -> [DescriptorPublicKey; 5] {
        std::array::from_fn(|_| fresh(Network::Regtest))
    }

    fn build(k: [DescriptorPublicKey; 5]) -> Result<SavingsKeys, Error> {
        let [p, d, dr, h, heir] = k;
        SavingsKeys::new(p, d, dr, h, heir)
    }

    #[test]
    fn accepts_five_fresh_test_keys() {
        let k = five();
        let keys = build(k.clone()).unwrap();
        assert_eq!(keys.get(Role::Helper), &k[3]);
    }

    #[test]
    fn rejects_a_repeated_key() {
        let mut k = five();
        k[4] = k[3].clone(); // HEIR reuses HELPER's key
        assert!(matches!(build(k), Err(Error::DuplicateKey(Role::Helper, Role::Heir))));
    }

    #[test]
    fn rejects_device_reusing_its_key_for_the_delayed_path() {
        let mut k = five();
        k[2] = k[1].clone();
        assert!(matches!(build(k), Err(Error::DuplicateKey(Role::Device, Role::DeviceDelayed))));
    }

    #[test]
    fn rejects_a_mainnet_key() {
        let mut k = five();
        k[0] = fresh(Network::Bitcoin);
        assert!(matches!(build(k), Err(Error::MainnetKey(Role::Phone))));
    }

    #[test]
    fn rejects_a_key_whose_receive_and_change_paths_are_the_same() {
        // Found by fuzzing: `<1;1>` passed the shape check, then collapsed to
        // a single path inside the descriptor and crashed SavingsPolicy::new.
        let mut k = five();
        let same_twice = k[2].to_string().replace("/<0;1>/*", "/<1;1>/*");
        k[2] = DescriptorPublicKey::from_str(&same_twice).unwrap();
        assert!(matches!(build(k), Err(Error::KeyShape(Role::DeviceDelayed))));
    }

    #[test]
    fn rejects_a_key_without_receive_and_change_paths() {
        let mut k = five();
        let single_path = k[1].to_string().replace("/<0;1>/*", "/0/*");
        k[1] = DescriptorPublicKey::from_str(&single_path).unwrap();
        assert!(matches!(build(k), Err(Error::KeyShape(Role::Device))));
    }
}
