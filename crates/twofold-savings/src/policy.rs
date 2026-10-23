use std::str::FromStr;

use crate::bitcoin::bip32::{ChainCode, ChildNumber, Fingerprint, Xpub};
use crate::bitcoin::hashes::{Hash, sha256};
use crate::bitcoin::secp256k1::PublicKey;
use crate::bitcoin::Network;
use crate::miniscript::descriptor::{Descriptor, DescriptorPublicKey, TapTree};
use crate::miniscript::policy::{Liftable, semantic::Policy};
use crate::{Delays, Error, Role, SavingsKeys};

/// BIP 341's "nothing up my sleeve" point H. Nobody knows its private key,
/// so a Taproot output whose internal key is derived from H has no key-path
/// spend; every spend must use one of the script leaves.
pub const NUMS_POINT: &str = "0250929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0";

/// The Savings wallet rules as a Taproot descriptor:
///
/// ```text
/// tr(UNSPENDABLE, { multi_a(2, PHONE, DEVICE, HELPER),
///                   { and_v(v:pk(DEVICE_R), older(device_delay)),
///                     and_v(v:pk(HEIR),     older(heir_delay)) } })
/// ```
///
/// The 2-of-3 leaf sits highest in the tree because it is the everyday path
/// and so gets the shortest proof.
#[derive(Clone, Debug)]
pub struct SavingsPolicy {
    keys: SavingsKeys,
    delays: Delays,
    network: Network,
    descriptor: Descriptor<DescriptorPublicKey>,
}

impl SavingsPolicy {
    /// Builds the descriptor. Refuses mainnet.
    pub fn new(keys: SavingsKeys, delays: Delays, network: Network) -> Result<Self, Error> {
        if network == Network::Bitcoin {
            return Err(Error::Mainnet);
        }
        let k = |role| keys.get(role).to_string();
        // Build the tree with H itself as a stand-in internal key, then swap in
        // the internal key computed from that tree.
        let draft = format!(
            "tr({NUMS_POINT},{{multi_a(2,{phone},{device},{helper}),{{and_v(v:pk({device_r}),older({dd})),and_v(v:pk({heir}),older({hd}))}}}})",
            phone = k(Role::Phone),
            device = k(Role::Device),
            helper = k(Role::Helper),
            device_r = k(Role::DeviceDelayed),
            heir = k(Role::Heir),
            dd = delays.device(),
            hd = delays.heir(),
        );
        let Descriptor::Tr(draft) = Descriptor::<DescriptorPublicKey>::from_str(&draft)? else {
            unreachable!("built as tr()")
        };
        let tree = draft.tap_tree().clone().expect("built with a tree");
        let internal = unspendable_internal_key(&tree).expect("SavingsKeys only holds multipath xpubs");
        let descriptor = Descriptor::new_tr(internal, Some(tree))?;
        descriptor.sanity_check()?;
        Ok(Self { keys, delays, network, descriptor })
    }

    pub fn keys(&self) -> &SavingsKeys {
        &self.keys
    }

    pub fn delays(&self) -> Delays {
        self.delays
    }

    pub fn network(&self) -> Network {
        self.network
    }

    /// The full descriptor, with receive and change paths written as `<0;1>`.
    pub fn descriptor(&self) -> &Descriptor<DescriptorPublicKey> {
        &self.descriptor
    }

    /// The receive and change descriptors, in that order, for wallets (like
    /// BDK) that take them separately.
    pub fn receive_and_change(&self) -> (Descriptor<DescriptorPublicKey>, Descriptor<DescriptorPublicKey>) {
        let mut both = self.descriptor.clone().into_single_descriptors().expect("built from <0;1> keys").into_iter();
        (both.next().expect("receive"), both.next().expect("change"))
    }

    /// The wallet descriptor to back up or import into other software
    /// (Liana, Bitcoin Core, and others that take descriptors): public keys
    /// only, receive and change written as `<0;1>`, with its checksum.
    ///
    /// It holds no private keys, so it cannot spend, but it reveals every
    /// Savings address and balance; treat it as private.
    pub fn export_descriptor(&self) -> String {
        self.descriptor.to_string()
    }

    /// The same as [`Self::export_descriptor`], split into separate receive
    /// and change descriptors, each with its own checksum, for software that
    /// doesn't read `<0;1>`.
    pub fn export_receive_and_change(&self) -> (String, String) {
        let (receive, change) = self.receive_and_change();
        (receive.to_string(), change.to_string())
    }

    /// Whether the holders of `signers` could spend a Savings coin that has
    /// had `age_blocks` confirmations, according to the descriptor itself.
    ///
    /// This reads the rules back out of the compiled descriptor rather than
    /// restating them, so it catches mistakes in how the descriptor is built.
    /// The unspendable internal key never signs.
    pub fn can_spend(&self, signers: &[Role], age_blocks: u16) -> bool {
        let keys: Vec<&DescriptorPublicKey> = signers.iter().map(|r| self.keys.get(*r)).collect();
        let lifted = self.descriptor.lift().expect("a Taproot miniscript descriptor always lifts");
        satisfied(&lifted, &keys, age_blocks)
    }
}

fn satisfied(policy: &Policy<DescriptorPublicKey>, signers: &[&DescriptorPublicKey], age: u16) -> bool {
    match policy {
        Policy::Trivial => true,
        Policy::Key(k) => signers.contains(&k),
        Policy::Older(lock) => lock.is_height_locked() && u32::from(age) >= lock.to_consensus_u32(),
        Policy::Thresh(t) => t.iter().filter(|sub| satisfied(sub, signers, age)).count() >= t.k(),
        // Absolute locktimes and hash locks are not part of Savings; treat
        // them as unsatisfiable so their appearance can only make tests fail.
        Policy::Unsatisfiable
        | Policy::After(_)
        | Policy::Sha256(_)
        | Policy::Hash256(_)
        | Policy::Ripemd160(_)
        | Policy::Hash160(_) => false,
    }
}

/// The unspendable Taproot internal key for `tree`, built the way Liana
/// builds it, so Liana recognises it as unspendable and imports the wallet.
///
/// It's an extended key whose public key is the NUMS point H and whose chain
/// code is SHA256 of the public key of every key in the tree, concatenated
/// in tree order: leaves in order, and keys in order within each leaf. A key
/// that appears in two leaves counts twice. For Savings, that's PHONE,
/// DEVICE, HELPER (the 2-of-3 leaf), then DEVICE_R, then HEIR. The network
/// is the first key's. The key is used as `xpub/<0;1>/*`, with no origin.
/// (The convention is from
/// <https://delvingbitcoin.org/t/unspendable-keys-in-descriptors/304/21>.)
///
/// Deriving from H keeps the key unspendable at every index, while the chain
/// code makes each wallet's internal keys different, so spends from
/// different wallets don't share an internal key. Anyone with the
/// descriptor's public keys can recompute it and check it.
///
/// `None` if the tree holds any key that isn't a multipath xpub.
pub fn unspendable_internal_key(tree: &TapTree<DescriptorPublicKey>) -> Option<DescriptorPublicKey> {
    let mut preimage = Vec::new();
    let mut network = None;
    for (_, ms) in tree.iter() {
        for key in ms.iter_pk() {
            let DescriptorPublicKey::MultiXPub(k) = key else { return None };
            network.get_or_insert(k.xkey.network);
            preimage.extend_from_slice(&k.xkey.public_key.serialize());
        }
    }
    let xpub = Xpub {
        network: network?,
        depth: 0,
        parent_fingerprint: Fingerprint::default(),
        child_number: ChildNumber::from_normal_idx(0).expect("0 is a valid index"),
        public_key: PublicKey::from_str(NUMS_POINT).expect("H is a valid point"),
        chain_code: ChainCode::from(sha256::Hash::hash(&preimage).to_byte_array()),
    };
    DescriptorPublicKey::from_str(&format!("{xpub}/<0;1>/*")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::test_keys::fresh;
    use crate::miniscript::descriptor::TapTree;

    use Role::{Device as D, DeviceDelayed as DR, Helper as H, Heir, Phone as P};

    fn policy(delays: Delays) -> SavingsPolicy {
        let [p, d, dr, h, heir] = std::array::from_fn(|_| fresh(Network::Regtest));
        let keys = SavingsKeys::new(p, d, dr, h, heir).unwrap();
        SavingsPolicy::new(keys, delays, Network::Regtest).unwrap()
    }

    /// Ages worth checking for a given set of delays: zero, each side of each
    /// delay, and the 65,535-block cap.
    fn ages(d: Delays) -> [u16; 6] {
        [0, d.device() - 1, d.device(), d.heir() - 1, d.heir(), u16::MAX]
    }

    #[test]
    fn matches_the_savings_rules_for_every_signer_set_and_age() {
        for delays in [Delays::default(), Delays::new(10, 20).unwrap()] {
            let policy = policy(delays);
            for mask in 0u8..32 {
                let signers: Vec<Role> =
                    Role::ALL.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, r)| *r).collect();
                let has = |r| signers.contains(&r);
                for age in ages(delays) {
                    // The rules, written out independently of the descriptor.
                    let two_of_three = [P, D, H].iter().filter(|r| has(**r)).count() >= 2;
                    let device_late = has(DR) && age >= delays.device();
                    let heir_late = has(Heir) && age >= delays.heir();
                    let expected = two_of_three || device_late || heir_late;
                    assert_eq!(policy.can_spend(&signers, age), expected, "signers {signers:?} at age {age}");
                }
            }
        }
    }

    #[test]
    fn phone_alone_can_never_spend() {
        let policy = policy(Delays::default());
        assert!(ages(policy.delays()).iter().all(|&age| !policy.can_spend(&[P], age)));
    }

    #[test]
    fn helper_alone_can_never_spend() {
        let policy = policy(Delays::default());
        assert!(ages(policy.delays()).iter().all(|&age| !policy.can_spend(&[H], age)));
    }

    #[test]
    fn device_can_spend_alone_only_from_its_delay() {
        let policy = policy(Delays::default());
        let d = policy.delays().device();
        assert!(!policy.can_spend(&[DR], d - 1));
        assert!(policy.can_spend(&[DR], d));
    }

    #[test]
    fn device_with_both_its_keys_cannot_spend_before_its_delay() {
        let policy = policy(Delays::default());
        let d = policy.delays().device();
        assert!(!policy.can_spend(&[D, DR], 0));
        assert!(!policy.can_spend(&[D, DR], d - 1));
        assert!(policy.can_spend(&[D, DR], d));
    }

    #[test]
    fn heir_can_spend_alone_only_from_its_delay() {
        let policy = policy(Delays::default());
        let h = policy.delays().heir();
        assert!(!policy.can_spend(&[Heir], h - 1));
        assert!(policy.can_spend(&[Heir], h));
    }

    #[test]
    fn heir_with_phone_or_helper_cannot_spend_before_the_heir_delay() {
        let policy = policy(Delays::default());
        let h = policy.delays().heir();
        for other in [P, H] {
            for age in [0, policy.delays().device(), h - 1] {
                assert!(!policy.can_spend(&[Heir, other], age), "HEIR + {other} at age {age}");
            }
            assert!(policy.can_spend(&[Heir, other], h));
        }
    }

    #[test]
    fn heir_with_device_cannot_spend_before_the_device_delay() {
        // After the device delay DEVICE can spend alone, so HEIR adds nothing;
        // before it, HEIR + DEVICE (with either or both device keys) is refused.
        let policy = policy(Delays::default());
        let d = policy.delays().device();
        for device_keys in [&[D][..], &[DR], &[D, DR]] {
            let signers: Vec<Role> = device_keys.iter().copied().chain([Heir]).collect();
            assert!(!policy.can_spend(&signers, 0), "{signers:?}");
            assert!(!policy.can_spend(&signers, d - 1), "{signers:?}");
        }
    }

    #[test]
    fn internal_key_is_derived_from_the_nums_point() {
        let policy = policy(Delays::default());
        let Descriptor::Tr(tr) = policy.descriptor() else { panic!("not Taproot") };
        let DescriptorPublicKey::MultiXPub(internal) = tr.internal_key() else { panic!("not an xpub") };
        assert_eq!(internal.xkey.public_key, PublicKey::from_str(NUMS_POINT).unwrap());
        // And it is not one of the five Savings keys.
        assert!(Role::ALL.iter().all(|r| policy.keys().get(*r) != tr.internal_key()));
    }

    /// Public test keys from Liana v15.0's own tests
    /// (liana/src/descriptors/mod.rs), as PHONE, DEVICE, DEVICE_R, HELPER,
    /// HEIR.
    const LIANA_TEST_KEYS: [&str; 5] = [
        "[37435aca/48'/1'/0'/2']tpubDEcZc7Um5KPZLF11qLYNgcteH1PH7CSKtTFTuTYuL6jqsP73gBHvLY42FuyvkTWN8C4q1YiDAUAeEuA78CDuuD2x17UKJymkBacCJz8P6NU/<0;1>/*",
        "[05813578/48'/1'/0'/2']tpubDEymPgUFZDLzEePYsai2ZXc9ntsnZBohJvGuHgpZsY5bsUDJMD6de6tevVd1z1EdGPYLdNaPvD4Ck7NgcteYDUPWwExvscfaSUu19k48Mvp/<0;1>/*",
        "[172ba1bc/48'/1'/0'/2']tpubDEgTZEAraUrKmnbyKJuXYGFPzNCm82bjMqd2GRy2HKviJ1moLtEZrHoUeG2o6uyWLEGx4yBWpctAmxcBx1b5nrrrBo5LjskRxRMDmwkuKxq/<0;1>/*",
        "[da2ee873/48'/1'/0'/2']tpubDEbXY6RbN9mxAvQW797WxReGGkrdyRfdYcehVVaQQcQ3kyfhxSMcnU9qGpUVRHXXALvBtc99jcuxx5tkzcLaJbAukSNpP9h2ti4XFRosv1g/<0;1>/*",
        "[3b1913e1/48'/1'/0'/2']tpubDFeZ2ezf4VUuTnjdhxJ1DKhLa2t6vzXZNz8NnEgeT2PN4pPqTCTeWUcaxKHPJcf1C8WzkLA71zSjDwuo4zqu4kkiL91ZUmJydC8f1gx89wM/<0;1>/*",
    ];

    #[test]
    fn internal_key_matches_lianas_own_code_for_the_savings_tree() {
        // Produced by Liana v15.0's own `liana::descriptors::unspendable_internal_key`
        // for the Savings tree built from LIANA_TEST_KEYS, which Liana's
        // `LianaDescriptor::from_str` then accepted. The delays don't affect it.
        const FROM_LIANA: &str = "tpubD6NzVbkrYhZ4WVca1q6LYRsqpKEgWEAQP1DEDYnmFhSGjNwTtDQSEvPXVrtjx8MGesG2LWpkU9vWeiwrsgviRw3fk2nFRYLb42QfRE6oKMW/<0;1>/*";
        let [p, d, dr, h, heir] = LIANA_TEST_KEYS.map(|k| DescriptorPublicKey::from_str(k).unwrap());
        let keys = SavingsKeys::new(p, d, dr, h, heir).unwrap();
        for delays in [Delays::default(), Delays::new(10, 20).unwrap()] {
            let policy = SavingsPolicy::new(keys.clone(), delays, Network::Regtest).unwrap();
            let Descriptor::Tr(tr) = policy.descriptor() else { panic!("not Taproot") };
            assert_eq!(tr.internal_key().to_string(), FROM_LIANA);
        }
    }

    #[test]
    fn internal_key_matches_lianas_published_test_vectors() {
        // Descriptors from Liana v15.0's tests (liana/src/descriptors/mod.rs:
        // `descriptor_unspendable_internal_key` and the test just before
        // "TODO: test error conditions of deserialization"), whose internal keys
        // Liana's tests check are its deterministic unspendable key. Both use
        // Liana's convention of one xpub on two paths (<0;1> and <2;3>), so a
        // key appears twice in the tree and counts twice.
        const VECTORS: [&str; 2] = [
            "tr(tpubD6NzVbkrYhZ4YdBUPkUhDYj6Sd1QK8vgiCf5RwHnAnSNK5ozemAZzPTYZbgQq4diod7oxFJJYGa8FNRHzRo7URkixzQTuudh38xRRdSc4Hu/<0;1>/*,{and_v(v:multi_a(1,[ffd63c8d/48'/1'/0'/2']tpubDExA3EC3iAsPxPhFn4j6gMiVup6V2eH3qKyk69RcTc9TTNRfFYVPad8bJD5FCHVQxyBT4izKsvr7Btd2R4xmQ1hZkvsqGBaeE82J71uTK4N/<2;3>/*,[da2ee873/48'/1'/0'/2']tpubDEbXY6RbN9mxAvQW797WxReGGkrdyRfdYcehVVaQQcQ3kyfhxSMcnU9qGpUVRHXXALvBtc99jcuxx5tkzcLaJbAukSNpP9h2ti4XFRosv1g/<2;3>/*),older(2)),multi_a(2,[ffd63c8d/48'/1'/0'/2']tpubDExA3EC3iAsPxPhFn4j6gMiVup6V2eH3qKyk69RcTc9TTNRfFYVPad8bJD5FCHVQxyBT4izKsvr7Btd2R4xmQ1hZkvsqGBaeE82J71uTK4N/<0;1>/*,[da2ee873/48'/1'/0'/2']tpubDEbXY6RbN9mxAvQW797WxReGGkrdyRfdYcehVVaQQcQ3kyfhxSMcnU9qGpUVRHXXALvBtc99jcuxx5tkzcLaJbAukSNpP9h2ti4XFRosv1g/<0;1>/*)})",
            "tr(tpubD6NzVbkrYhZ4XokGX5s1FcKj2ozqibXqXc79NkzEbNQvT1j9F7YWe3fPp3eDeMVypHXocGWUkVaC6ZUqMqyQRS27XJcujQLbrqzpoYYLGW5/<0;1>/*,{and_v(v:multi_a(1,[37435aca/48'/1'/0'/2']tpubDEcZc7Um5KPZLF11qLYNgcteH1PH7CSKtTFTuTYuL6jqsP73gBHvLY42FuyvkTWN8C4q1YiDAUAeEuA78CDuuD2x17UKJymkBacCJz8P6NU/<2;3>/*,[05813578/48'/1'/0'/2']tpubDEymPgUFZDLzEePYsai2ZXc9ntsnZBohJvGuHgpZsY5bsUDJMD6de6tevVd1z1EdGPYLdNaPvD4Ck7NgcteYDUPWwExvscfaSUu19k48Mvp/<2;3>/*),older(52596)),and_v(v:pk([37435aca/48'/1'/0'/2']tpubDEcZc7Um5KPZLF11qLYNgcteH1PH7CSKtTFTuTYuL6jqsP73gBHvLY42FuyvkTWN8C4q1YiDAUAeEuA78CDuuD2x17UKJymkBacCJz8P6NU/<0;1>/*),pk([05813578/48'/1'/0'/2']tpubDEymPgUFZDLzEePYsai2ZXc9ntsnZBohJvGuHgpZsY5bsUDJMD6de6tevVd1z1EdGPYLdNaPvD4Ck7NgcteYDUPWwExvscfaSUu19k48Mvp/<0;1>/*))})#ffdmpxzr",
        ];
        for vector in VECTORS {
            let Descriptor::Tr(tr) = Descriptor::<DescriptorPublicKey>::from_str(vector).unwrap() else {
                panic!("not Taproot")
            };
            let tree = tr.tap_tree().as_ref().unwrap();
            assert_eq!(unspendable_internal_key(tree).as_ref(), Some(tr.internal_key()), "{vector}");
        }
    }

    #[test]
    fn internal_key_follows_tree_order_not_role_order() {
        // The order we used before Liana compatibility (PHONE, DEVICE, DEVICE_R,
        // HELPER, HEIR) gives a different key, which Liana rejects as a
        // spendable key. Guard against going back to it.
        let [p, d, dr, h, heir] = LIANA_TEST_KEYS.map(|k| DescriptorPublicKey::from_str(k).unwrap());
        let keys = SavingsKeys::new(p, d, dr, h, heir).unwrap();
        let policy = SavingsPolicy::new(keys.clone(), Delays::default(), Network::Regtest).unwrap();
        let Descriptor::Tr(tr) = policy.descriptor() else { panic!("not Taproot") };
        let xpub = |k: &DescriptorPublicKey| match k {
            DescriptorPublicKey::MultiXPub(m) => m.xkey,
            _ => unreachable!(),
        };
        let role_order: Vec<u8> =
            Role::ALL.iter().flat_map(|r| xpub(keys.get(*r)).public_key.serialize()).collect();
        let old = sha256::Hash::hash(&role_order).to_byte_array();
        assert_ne!(xpub(tr.internal_key()).chain_code.to_bytes(), old);
    }

    #[test]
    fn has_exactly_three_leaves_with_the_two_of_three_on_top() {
        let policy = policy(Delays::default());
        let Descriptor::Tr(tr) = policy.descriptor() else { panic!("not Taproot") };
        let leaves: Vec<(u8, String)> = tr.iter_scripts().map(|(depth, ms)| (depth, ms.to_string())).collect();
        assert_eq!(leaves.len(), 3);
        assert_eq!(leaves[0].0, 1);
        assert!(leaves[0].1.starts_with("multi_a(2,"));
        assert!(leaves[1].1.ends_with("older(39420))"));
        assert!(leaves[2].1.ends_with("older(61320))"));
        assert!(matches!(tr.tap_tree(), Some(TapTree::Tree { .. })));
    }

    #[test]
    fn descriptor_round_trips_through_text() {
        let policy = policy(Delays::new(10, 20).unwrap());
        let text = policy.descriptor().to_string();
        assert!(text.contains('#'), "has a checksum");
        assert_eq!(&Descriptor::<DescriptorPublicKey>::from_str(&text).unwrap(), policy.descriptor());
    }

    #[test]
    fn splits_into_receive_and_change() {
        let policy = policy(Delays::default());
        let (receive, change) = policy.receive_and_change();
        assert!(receive.to_string().contains("/0/*"));
        assert!(change.to_string().contains("/1/*"));
        assert!(!receive.is_multipath() && !change.is_multipath());
    }

    #[test]
    fn export_is_public_checksummed_and_parses_back() {
        let policy = policy(Delays::default());
        let exported = policy.export_descriptor();
        assert!(!exported.contains("prv"), "no private keys");
        let (body, checksum) = exported.split_once('#').expect("has a checksum");
        assert_eq!(checksum.len(), 8);
        assert!(body.starts_with("tr(") && body.contains("/<0;1>/*"));
        assert_eq!(&Descriptor::<DescriptorPublicKey>::from_str(&exported).unwrap(), policy.descriptor());

        let (receive, change) = policy.export_receive_and_change();
        assert!(receive.contains("/0/*") && change.contains("/1/*"));
        assert!(!receive.contains("<0;1>") && receive.contains('#') && change.contains('#'));
    }

    #[test]
    fn refuses_mainnet() {
        let [p, d, dr, h, heir] = std::array::from_fn(|_| fresh(Network::Regtest));
        let keys = SavingsKeys::new(p, d, dr, h, heir).unwrap();
        assert!(matches!(SavingsPolicy::new(keys, Delays::default(), Network::Bitcoin), Err(Error::Mainnet)));
    }
}
