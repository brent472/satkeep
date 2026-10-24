//! A phone key restored from its backup approves a real Savings spend on
//! regtest; a key restored from another wallet's backup doesn't.
//!
//! The security device is simulated in software. Keys and devices are made
//! fresh in each test, and nothing is printed.

mod common;

use common::{Holder, Node, Path, Savings};
use satkeep_savings::backup::{self, HeirSettings, SavingsWallet, SimulatedDevice, WalletSetup, setup};
use satkeep_savings::bitcoin::Amount;
use satkeep_savings::{Delays, Error, HolderKey};

const PIN: &str = "2468";
const CREATED: u64 = 1_792_713_600;

/// The wallet setup the backup carries: this Savings wallet, with the
/// beneficiary's settings (the check-in values are only recorded here).
fn setup_of(savings: &Savings) -> WalletSetup {
    WalletSetup {
        network: setup::Network::Regtest,
        pocket: None,
        savings: vec![SavingsWallet {
            descriptor: savings.policy.export_descriptor(),
            birthday: 0,
            heir: Some(HeirSettings { heir_delay: 20, checkin_interval: 10, checkin_margin: 5, refresh_window: None }),
        }],
    }
}

/// Backs up `phone` to a fresh simulated device, then restores it from the
/// file with that device, as a new phone would.
fn back_up_and_restore(phone: &HolderKey, setup: &WalletSetup) -> HolderKey {
    let mut device = SimulatedDevice::new(PIN);
    device.enter_pin(PIN).unwrap();
    let file = backup::make_backup(phone, setup, backup::new_wallet_id(), CREATED, &mut device).unwrap();
    device.enter_pin(PIN).unwrap();
    let restored = backup::restore(&file, &mut device).unwrap();
    assert_eq!(restored.wallet_setup, *setup);
    restored.phone
}

fn funded(node: &Node) -> Savings {
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(node, Amount::from_btc(1.0).unwrap());
    savings
}

#[test]
fn a_restored_phone_key_approves_a_2_of_3_spend_with_the_helper() {
    let node = Node::start();
    let mut savings = funded(&node);
    let restored = back_up_and_restore(savings.phone_key(), &setup_of(&savings));

    let mut psbt = savings.build(Path::TwoOfThree, &node.new_address()).unwrap();
    assert!(restored.sign(&mut psbt).unwrap() > 0, "the restored key signs");
    savings.sign(&mut psbt, &[Holder::Helper]);
    let tx = savings.finalize(&psbt).expect("restored PHONE + HELPER is 2 of 3");
    assert_eq!(node.accepts(&tx), Ok(()));
    node.broadcast_and_confirm(&tx);
    savings.sync(&node);
    assert_eq!(savings.wallet.balance().total(), Amount::ZERO);
}

#[test]
fn a_key_restored_from_another_wallets_backup_cannot_approve() {
    let node = Node::start();
    let mut savings = funded(&node);
    let other = Savings::new(Delays::with_heir(20).unwrap());
    let stranger = back_up_and_restore(other.phone_key(), &setup_of(&other));

    let mut psbt = savings.build(Path::TwoOfThree, &node.new_address()).unwrap();
    assert_eq!(stranger.sign(&mut psbt).unwrap(), 0, "a key from another wallet adds no signature");
    savings.sign(&mut psbt, &[Holder::Helper]);
    assert!(matches!(savings.finalize(&psbt), Err(Error::NotEnoughApprovals)));
}
