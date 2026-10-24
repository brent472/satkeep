use std::fmt;

use crate::Error;
use crate::bitcoin::NetworkKind;
use crate::miniscript::descriptor::{DescriptorPublicKey, Wildcard};

/// Who holds a key in the Savings policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Phone,
    Device,
    Helper,
    /// The beneficiary. Only in Savings that has one.
    Heir,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Phone, Role::Device, Role::Helper, Role::Heir];
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Phone => "PHONE",
            Role::Device => "DEVICE",
            Role::Helper => "HELPER",
            Role::Heir => "HEIR",
        })
    }
}

/// The public keys of a Savings wallet: PHONE, DEVICE and HELPER, plus HEIR
/// once a beneficiary is added.
///
/// Each key must be a test-network extended public key with a receive and a
/// change path (`…/<0;1>/*`), and no two keys may share an extended key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavingsKeys {
    phone: DescriptorPublicKey,
    device: DescriptorPublicKey,
    helper: DescriptorPublicKey,
    heir: Option<DescriptorPublicKey>,
}

impl SavingsKeys {
    pub fn new(
        phone: DescriptorPublicKey,
        device: DescriptorPublicKey,
        helper: DescriptorPublicKey,
        heir: Option<DescriptorPublicKey>,
    ) -> Result<Self, Error> {
        let keys = Self { phone, device, helper, heir };
        let present: Vec<(Role, &DescriptorPublicKey)> =
            Role::ALL.iter().filter_map(|r| keys.get(*r).map(|k| (*r, k))).collect();
        for (role, key) in &present {
            check_shape(*role, key)?;
        }
        for (i, (a, ka)) in present.iter().enumerate() {
            for (b, kb) in &present[i + 1..] {
                if xpub(ka) == xpub(kb) {
                    return Err(Error::DuplicateKey(*a, *b));
                }
            }
        }
        Ok(keys)
    }

    /// The key for `role`; `None` only for HEIR in Savings with no
    /// beneficiary.
    pub fn get(&self, role: Role) -> Option<&DescriptorPublicKey> {
        match role {
            Role::Phone => Some(&self.phone),
            Role::Device => Some(&self.device),
            Role::Helper => Some(&self.helper),
            Role::Heir => self.heir.as_ref(),
        }
    }

    /// The same PHONE, DEVICE and HELPER with `heir` added, for moving
    /// Savings to a wallet with a beneficiary.
    pub fn with_heir(&self, heir: DescriptorPublicKey) -> Result<Self, Error> {
        Self::new(self.phone.clone(), self.device.clone(), self.helper.clone(), Some(heir))
    }

    /// Whether `other` has the same PHONE, DEVICE and HELPER keys.
    pub fn same_two_of_three(&self, other: &SavingsKeys) -> bool {
        self.phone == other.phone && self.device == other.device && self.helper == other.helper
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

    fn four() -> [DescriptorPublicKey; 4] {
        std::array::from_fn(|_| fresh(Network::Regtest))
    }

    fn build(k: [DescriptorPublicKey; 4]) -> Result<SavingsKeys, Error> {
        let [p, d, h, heir] = k;
        SavingsKeys::new(p, d, h, Some(heir))
    }

    #[test]
    fn accepts_fresh_test_keys_with_or_without_an_heir() {
        let k = four();
        let keys = build(k.clone()).unwrap();
        assert_eq!(keys.get(Role::Helper), Some(&k[2]));
        assert_eq!(keys.get(Role::Heir), Some(&k[3]));
        let [p, d, h, _] = k;
        let plain = SavingsKeys::new(p, d, h, None).unwrap();
        assert_eq!(plain.get(Role::Heir), None);
    }

    #[test]
    fn adding_an_heir_keeps_the_other_keys() {
        let [p, d, h, heir] = four();
        let plain = SavingsKeys::new(p.clone(), d.clone(), h.clone(), None).unwrap();
        let with = plain.with_heir(heir.clone()).unwrap();
        assert_eq!(with, SavingsKeys::new(p, d, h, Some(heir)).unwrap());
        assert!(plain.same_two_of_three(&with));
        let phone = plain.get(Role::Phone).unwrap().clone();
        assert!(matches!(plain.with_heir(phone), Err(Error::DuplicateKey(Role::Phone, Role::Heir))));
    }

    #[test]
    fn rejects_a_repeated_key() {
        let mut k = four();
        k[3] = k[2].clone(); // HEIR reuses HELPER's key
        assert!(matches!(build(k), Err(Error::DuplicateKey(Role::Helper, Role::Heir))));
        let mut k = four();
        k[1] = k[0].clone();
        assert!(matches!(build(k), Err(Error::DuplicateKey(Role::Phone, Role::Device))));
    }

    #[test]
    fn rejects_a_mainnet_key() {
        let mut k = four();
        k[0] = fresh(Network::Bitcoin);
        assert!(matches!(build(k), Err(Error::MainnetKey(Role::Phone))));
        let mut k = four();
        k[3] = fresh(Network::Bitcoin);
        assert!(matches!(build(k), Err(Error::MainnetKey(Role::Heir))));
    }

    #[test]
    fn rejects_a_key_whose_receive_and_change_paths_are_the_same() {
        // Found by fuzzing: `<1;1>` passed the shape check, then collapsed to
        // a single path inside the descriptor and crashed SavingsPolicy::new.
        let mut k = four();
        let same_twice = k[2].to_string().replace("/<0;1>/*", "/<1;1>/*");
        k[2] = DescriptorPublicKey::from_str(&same_twice).unwrap();
        assert!(matches!(build(k), Err(Error::KeyShape(Role::Helper))));
    }

    #[test]
    fn rejects_a_key_without_receive_and_change_paths() {
        let mut k = four();
        let single_path = k[1].to_string().replace("/<0;1>/*", "/0/*");
        k[1] = DescriptorPublicKey::from_str(&single_path).unwrap();
        assert!(matches!(build(k), Err(Error::KeyShape(Role::Device))));
    }
}
