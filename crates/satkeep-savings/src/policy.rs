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

/// The Savings wallet rules as a descriptor, in one of two shapes.
///
/// With no beneficiary yet, plain 2 of 3 in the standard P2WSH multisig
/// format, with no delayed path. Bitcoin Core, Sparrow, Electrum and Nunchuk
/// all read it, which Liana, which requires a delayed path, can't:
///
/// ```text
/// wsh(sortedmulti(2, PHONE, DEVICE, HELPER))
/// ```
///
/// With a beneficiary, a Taproot policy where HEIR alone can also spend once
/// a coin has waited the heir delay. Liana and Bitcoin Core read it:
///
/// ```text
/// tr(UNSPENDABLE, { multi_a(2, PHONE, DEVICE, HELPER),
///                   and_v(v:pk(HEIR), older(heir_delay)) })
/// ```
///
/// Adding a beneficiary later means a new wallet in the second shape and one
/// transaction moving Savings to it ([`crate::migrate`]).
#[derive(Clone, Debug)]
pub struct SavingsPolicy {
    keys: SavingsKeys,
    delays: Delays,
    network: Network,
    descriptor: Descriptor<DescriptorPublicKey>,
}

impl SavingsPolicy {
    /// Builds the descriptor. `delays` must have an heir delay exactly when
    /// `keys` has a HEIR key. Refuses mainnet.
    pub fn new(keys: SavingsKeys, delays: Delays, network: Network) -> Result<Self, Error> {
        if network == Network::Bitcoin {
            return Err(Error::Mainnet);
        }
        let k = |role| keys.get(role).expect("PHONE, DEVICE and HELPER are always there").to_string();
        let (phone, device, helper) = (k(Role::Phone), k(Role::Device), k(Role::Helper));
        let descriptor = match (keys.get(Role::Heir), delays.heir()) {
            (None, None) => Descriptor::<DescriptorPublicKey>::from_str(&format!(
                "wsh(sortedmulti(2,{phone},{device},{helper}))"
            ))?,
            (Some(heir), Some(hd)) => {
                // Build the tree with H itself as a stand-in internal key, then
                // swap in the internal key computed from that tree.
                let draft = format!(
                    "tr({NUMS_POINT},{{multi_a(2,{phone},{device},{helper}),and_v(v:pk({heir}),older({hd}))}})"
                );
                let Descriptor::Tr(draft) = Descriptor::<DescriptorPublicKey>::from_str(&draft)? else {
                    unreachable!("built as tr()")
                };
                let tree = draft.tap_tree().clone().expect("built with a tree");
                let internal = unspendable_internal_key(&tree).ok_or(Error::InternalKey)?;
                Descriptor::new_tr(internal, Some(tree))?
            }
            _ => return Err(Error::HeirAndDelayMismatch),
        };
        descriptor.sanity_check()?;
        Ok(Self { keys, delays, network, descriptor })
    }

    pub fn keys(&self) -> &SavingsKeys {
        &self.keys
    }

    pub fn delays(&self) -> Delays {
        self.delays
    }

    /// Whether this Savings has a beneficiary, and so an heir path.
    pub fn has_heir(&self) -> bool {
        self.delays.heir().is_some()
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
    /// The unspendable internal key never signs. A role with no key in this
    /// wallet (HEIR, with no beneficiary) adds nothing.
    pub fn can_spend(&self, signers: &[Role], age_blocks: u16) -> bool {
        let keys: Vec<&DescriptorPublicKey> = signers.iter().filter_map(|r| self.keys.get(*r)).collect();
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
/// DEVICE, HELPER (the 2-of-3 leaf), then HEIR if there is one. The network
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

    use Role::{Device as D, Helper as H, Heir, Phone as P};

    const TEST_HEIR_DELAY: u16 = 20;

    fn with_heir(heir_delay: u16) -> SavingsPolicy {
        let [p, d, h, heir] = std::array::from_fn(|_| fresh(Network::Regtest));
        let keys = SavingsKeys::new(p, d, h, Some(heir)).unwrap();
        SavingsPolicy::new(keys, Delays::with_heir(heir_delay).unwrap(), Network::Regtest).unwrap()
    }

    fn plain() -> SavingsPolicy {
        let [p, d, h] = std::array::from_fn(|_| fresh(Network::Regtest));
        let keys = SavingsKeys::new(p, d, h, None).unwrap();
        SavingsPolicy::new(keys, Delays::none(), Network::Regtest).unwrap()
    }

    fn both_shapes() -> [SavingsPolicy; 3] {
        [plain(), with_heir(crate::DEFAULT_HEIR_DELAY), with_heir(TEST_HEIR_DELAY)]
    }

    /// Ages worth checking: zero, each side of the heir delay, and the
    /// 65,535-block cap.
    fn ages(policy: &SavingsPolicy) -> Vec<u16> {
        let mut ages = vec![0, 1, u16::MAX];
        if let Some(hd) = policy.delays().heir() {
            ages.extend([hd - 1, hd]);
        }
        ages
    }

    /// Every subset of the four roles.
    fn signer_sets() -> impl Iterator<Item = Vec<Role>> {
        (0u8..16).map(|mask| Role::ALL.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, r)| *r).collect())
    }

    #[test]
    fn matches_the_savings_rules_for_every_signer_set_and_age() {
        for policy in both_shapes() {
            for signers in signer_sets() {
                let has = |r| signers.contains(&r);
                for age in ages(&policy) {
                    // The rules, written out independently of the descriptor.
                    let two_of_three = [P, D, H].iter().filter(|r| has(**r)).count() >= 2;
                    let heir_late = has(Heir) && policy.delays().heir().is_some_and(|hd| age >= hd);
                    let expected = two_of_three || heir_late;
                    assert_eq!(policy.can_spend(&signers, age), expected, "signers {signers:?} at age {age}");
                }
            }
        }
    }

    #[test]
    fn no_single_key_but_heir_can_ever_spend_alone() {
        for policy in both_shapes() {
            for role in [P, D, H] {
                assert!(ages(&policy).iter().all(|&age| !policy.can_spend(&[role], age)), "{role} alone");
            }
        }
    }

    #[test]
    fn without_a_beneficiary_nothing_opens_with_time() {
        let policy = plain();
        assert!(!policy.has_heir());
        for signers in signer_sets() {
            let two = [P, D, H].iter().filter(|r| signers.contains(r)).count() >= 2;
            assert_eq!(policy.can_spend(&signers, u16::MAX), two, "{signers:?} at the 65,535-block cap");
        }
    }

    #[test]
    fn heir_can_spend_alone_only_from_its_delay() {
        let policy = with_heir(crate::DEFAULT_HEIR_DELAY);
        let h = policy.delays().heir().unwrap();
        assert!(!policy.can_spend(&[Heir], 0));
        assert!(!policy.can_spend(&[Heir], h - 1));
        assert!(policy.can_spend(&[Heir], h));
    }

    #[test]
    fn heir_with_any_one_other_key_cannot_spend_before_the_heir_delay() {
        let policy = with_heir(crate::DEFAULT_HEIR_DELAY);
        let h = policy.delays().heir().unwrap();
        for other in [P, D, H] {
            for age in [0, h / 2, h - 1] {
                assert!(!policy.can_spend(&[Heir, other], age), "HEIR + {other} at age {age}");
            }
            assert!(policy.can_spend(&[Heir, other], h));
        }
    }

    #[test]
    fn a_heir_key_and_an_heir_delay_come_together() {
        let [p, d, h, heir] = std::array::from_fn(|_| fresh(Network::Regtest));
        let plain_keys = SavingsKeys::new(p.clone(), d.clone(), h.clone(), None).unwrap();
        let heir_keys = SavingsKeys::new(p, d, h, Some(heir)).unwrap();
        let delay = Delays::with_heir(TEST_HEIR_DELAY).unwrap();
        assert!(matches!(SavingsPolicy::new(plain_keys, delay, Network::Regtest), Err(Error::HeirAndDelayMismatch)));
        assert!(matches!(SavingsPolicy::new(heir_keys, Delays::none(), Network::Regtest), Err(Error::HeirAndDelayMismatch)));
    }

    #[test]
    fn internal_key_is_derived_from_the_nums_point() {
        for policy in both_shapes().into_iter().filter(SavingsPolicy::has_heir) {
            let Descriptor::Tr(tr) = policy.descriptor() else { panic!("not Taproot") };
            let DescriptorPublicKey::MultiXPub(internal) = tr.internal_key() else { panic!("not an xpub") };
            assert_eq!(internal.xkey.public_key, PublicKey::from_str(NUMS_POINT).unwrap());
            // And it is not one of the Savings keys.
            assert!(Role::ALL.iter().all(|r| policy.keys().get(*r) != Some(tr.internal_key())));
        }
    }

    #[test]
    fn adding_an_heir_moves_to_taproot_and_changes_every_address() {
        let [p, d, h, heir] = std::array::from_fn(|_| fresh(Network::Regtest));
        let plain_keys = SavingsKeys::new(p, d, h, None).unwrap();
        let heir_keys = plain_keys.with_heir(heir).unwrap();
        let a = SavingsPolicy::new(plain_keys, Delays::none(), Network::Regtest).unwrap();
        let b = SavingsPolicy::new(heir_keys, Delays::with_heir(TEST_HEIR_DELAY).unwrap(), Network::Regtest).unwrap();
        assert!(matches!(a.descriptor(), Descriptor::Wsh(_)), "no beneficiary: P2WSH");
        assert!(matches!(b.descriptor(), Descriptor::Tr(_)), "with one: Taproot");
        assert_ne!(a.receive_and_change().0, b.receive_and_change().0);
    }

    #[test]
    fn internal_key_matches_lianas_published_test_vectors() {
        // Descriptors from Liana v15.0's tests (liana/src/descriptors/mod.rs:
        // `descriptor_unspendable_internal_key` and the test just before
        // "TODO: test error conditions of deserialization"), whose internal keys
        // Liana's tests check are its deterministic unspendable key. Both use
        // Liana's convention of one xpub on two paths (<0;1> and <2;3>), so a
        // key appears twice in the tree and counts twice. Whether Liana
        // accepts the Savings descriptor itself is checked by the exit drill.
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
    fn the_two_shapes_have_the_expected_leaves() {
        let plain = plain().export_descriptor();
        assert!(plain.starts_with("wsh(sortedmulti(2,") && plain.matches("/<0;1>/*").count() == 3, "{plain}");
        assert!(!plain.contains("older("), "no delayed path: {plain}");

        let policy = with_heir(crate::DEFAULT_HEIR_DELAY);
        let Descriptor::Tr(tr) = policy.descriptor() else { panic!("not Taproot") };
        let leaves: Vec<(u8, String)> = tr.iter_scripts().map(|(depth, ms)| (depth, ms.to_string())).collect();
        assert_eq!(leaves.len(), 2);
        assert!(leaves[0].1.starts_with("multi_a(2,"));
        assert!(leaves[1].1.ends_with("older(61320))"));
        assert!(!policy.export_descriptor().contains("older(39420)"), "no card-only path");
    }

    #[test]
    fn descriptor_round_trips_through_text() {
        for policy in both_shapes() {
            let text = policy.descriptor().to_string();
            assert!(text.contains('#'), "has a checksum");
            assert_eq!(&Descriptor::<DescriptorPublicKey>::from_str(&text).unwrap(), policy.descriptor());
        }
    }

    #[test]
    fn splits_into_receive_and_change() {
        for policy in both_shapes() {
            let (receive, change) = policy.receive_and_change();
            assert!(receive.to_string().contains("/0/*"));
            assert!(change.to_string().contains("/1/*"));
            assert!(!receive.is_multipath() && !change.is_multipath());
        }
    }

    #[test]
    fn export_is_public_checksummed_and_parses_back() {
        for policy in both_shapes() {
            let exported = policy.export_descriptor();
            // Piece by piece: a random tpub or checksum can contain "prv".
            assert!(!satkeep_backup::setup::holds_private_key(&exported), "no private keys");
            assert!(satkeep_backup::setup::holds_private_key(&exported.replacen("tpub", "tprv", 1)), "the check works");
            let (body, checksum) = exported.split_once('#').expect("has a checksum");
            assert_eq!(checksum.len(), 8);
            assert!((body.starts_with("tr(") || body.starts_with("wsh(sortedmulti(2,")) && body.contains("/<0;1>/*"));
            assert_eq!(&Descriptor::<DescriptorPublicKey>::from_str(&exported).unwrap(), policy.descriptor());

            let (receive, change) = policy.export_receive_and_change();
            assert!(receive.contains("/0/*") && change.contains("/1/*"));
            assert!(!receive.contains("<0;1>") && receive.contains('#') && change.contains('#'));
            assert!(![&receive, &change].into_iter().any(|d| satkeep_backup::setup::holds_private_key(d)));
        }
    }

    #[test]
    fn refuses_mainnet() {
        let [p, d, h] = std::array::from_fn(|_| fresh(Network::Regtest));
        let keys = SavingsKeys::new(p, d, h, None).unwrap();
        assert!(matches!(SavingsPolicy::new(keys, Delays::none(), Network::Bitcoin), Err(Error::Mainnet)));
    }
}
