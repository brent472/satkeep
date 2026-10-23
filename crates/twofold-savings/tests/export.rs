//! The exported descriptor means the same thing to Bitcoin Core as it does
//! to Twofold: Core, reading only the exported text, derives the same
//! addresses BDK hands out.

mod common;

use bdk_bitcoind_rpc::bitcoincore_rpc::RpcApi;
use bdk_wallet::KeychainKind;
use common::{Node, Savings};
use serde_json::{Value, json};
use twofold_savings::Delays;

#[test]
fn bitcoin_core_derives_the_same_addresses_from_the_export_in_both_shapes() {
    let node = Node::start();
    let heir = Delays::with_heir(twofold_savings::DEFAULT_HEIR_DELAY).unwrap();
    for savings in [Savings::new(Delays::none()), Savings::new(heir)] {
        let info: Value =
            node.rpc().call("getdescriptorinfo", &[json!(savings.policy.export_descriptor())]).unwrap();
        assert_eq!(info["isrange"], json!(true));
        assert_eq!(info["issolvable"], json!(true));
        assert_eq!(info["hasprivatekeys"], json!(false));

        let (receive, change) = savings.policy.export_receive_and_change();
        for (exported, keychain) in [(receive, KeychainKind::External), (change, KeychainKind::Internal)] {
            let core: Vec<String> = node.rpc().call("deriveaddresses", &[json!(exported), json!([0, 4])]).unwrap();
            let bdk: Vec<String> =
                (0..=4).map(|i| savings.wallet.peek_address(keychain, i).address.to_string()).collect();
            assert_eq!(core, bdk, "{keychain:?} addresses");
        }
    }
}
