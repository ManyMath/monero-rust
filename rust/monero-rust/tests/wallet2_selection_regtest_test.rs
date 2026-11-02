//! Needs `MONERO_REGTEST_RPC` and `WALLET2_SELECTION_STATE`, the state file that
//! `scripts/wallet2_selection_probe.py` and `scripts/wallet2_split_probe.py` write.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use std::collections::BTreeSet;

use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::monero_rpc::{HttpRpc, Rpc};
use monero_rust::native::{create_transaction_with_rpc, sweep_all_with_rpc, FeePolicy};
use monero_rust::scanner::{scan_blocks_batch, Lookahead};
use monero_rust::tx_prepare::{
    prepare_send_inputs_with_rpc, prepare_sweep_inputs_with_rpc,
    prepare_sweep_transactions_with_rpc, SweepSubaddresses,
};
use monero_rust::WalletOutput;

const XMR: u64 = 1_000_000_000_000;
const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;
const LOOKAHEAD: Lookahead = Lookahead {
    account: 1,
    subaddress: 10,
};
const MIN_HEIGHT_TO_SPEND: usize = 750;
const RUNS: usize = 32;

static REGTEST_CHAIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

type Picked = Vec<(u64, u32)>;

struct Probe {
    rpc: Rpc<HttpRpc>,
    state: serde_json::Value,
    recipient: String,
}

impl Probe {
    async fn open() -> Option<Probe> {
        let node = common::regtest_node()?;
        let Ok(state_path) = std::env::var("WALLET2_SELECTION_STATE") else {
            eprintln!("Skipping: WALLET2_SELECTION_STATE is unset");
            return None;
        };
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(state_path).expect("state file"))
                .expect("state json");
        let recipient = state["recipient"].as_str().expect("recipient").to_string();

        let rpc = HttpRpc::new(node).expect("regtest RPC");
        let _chain = REGTEST_CHAIN.lock().await;
        let height = rpc.get_height().await.expect("height");
        if height < MIN_HEIGHT_TO_SPEND {
            rpc.generate_blocks(&recipient, MIN_HEIGHT_TO_SPEND - height)
                .await
                .expect("mine decoys");
        }
        Some(Probe {
            rpc,
            state,
            recipient,
        })
    }

    fn seed(&self, wallet: &str) -> String {
        self.state["wallets"][wallet]
            .as_str()
            .unwrap_or_else(|| panic!("no seed for {wallet} in the state file"))
            .to_string()
    }

    async fn height(&self) -> u64 {
        self.rpc.get_height().await.expect("height") as u64
    }

    async fn scan_range(&self, wallet: &str, from_key: &str, to_key: &str) -> Vec<WalletOutput> {
        let seed = self.seed(wallet);
        let to = self.state[to_key].as_u64().expect(to_key);
        let mut next = self.state[from_key].as_u64().expect(from_key);
        let mut outputs = Vec::new();
        while next <= to {
            let batch = scan_blocks_batch(&self.rpc, next, &seed, NETWORK, LOOKAHEAD, false, "")
                .await
                .expect("scan");
            let last = batch.last().expect("a batch has blocks").block_height;
            for block in batch {
                if block.block_height <= to {
                    outputs.extend(block.outputs);
                }
            }
            next = last + 1;
        }
        outputs
    }

    async fn scan(&self, wallet: &str) -> Vec<WalletOutput> {
        self.scan_range(wallet, "scan_from", "scan_to").await
    }

    async fn selections(&self, outputs: &[WalletOutput], amount: u64) -> Vec<Vec<String>> {
        let height = self.height().await;
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            let prepared = prepare_send_inputs_with_rpc(
                &self.rpc,
                outputs,
                height,
                amount,
                1,
                None,
                None,
                0,
                FeePolicy::default(),
            )
            .await
            .expect("select");
            runs.push(prepared.spent_output_keys);
        }
        runs
    }

    async fn sweeps(&self, outputs: &[WalletOutput]) -> Vec<Vec<String>> {
        let height = self.height().await;
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            let prepared = prepare_sweep_inputs_with_rpc(
                &self.rpc,
                outputs,
                height,
                None,
                None,
                0,
                FeePolicy::default(),
            )
            .await
            .expect("prepare sweep");
            runs.push(prepared.spent_output_keys);
        }
        runs
    }

    async fn expect_selection(&self, wallet: &str, amount: u64, wallet2: Picked) {
        let outputs = self.scan(wallet).await;
        for keys in self.selections(&outputs, amount).await {
            assert_eq!(describe(&outputs, &keys), wallet2, "{wallet}");
        }
    }
}

fn describe(outputs: &[WalletOutput], keys: &[String]) -> Picked {
    let mut picked: Picked = keys
        .iter()
        .map(|key| {
            let output = outputs
                .iter()
                .find(|output| &output.output_key() == key)
                .expect("selected output is one of the wallet's");
            (
                output.amount,
                output.subaddress_index.map_or(0, |index| index.1),
            )
        })
        .collect();
    picked.sort();
    picked
}

#[tokio::test(flavor = "multi_thread")]
async fn second_input_stays_in_the_first_inputs_subaddress() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    probe
        .expect_selection("w_other_subaddress", XMR, vec![(3 * XMR, 0)])
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn inputs_come_from_one_subaddress_while_it_can_pay() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    let outputs = probe.scan("w_subaddress_order").await;
    let mut combinations = BTreeSet::new();
    for mut keys in probe.selections(&outputs, 5 * XMR / 2).await {
        assert_eq!(describe(&outputs, &keys), vec![(XMR, 1); 3]);
        keys.sort();
        combinations.insert(keys);
    }
    assert_eq!(
        combinations.len(),
        4,
        "which three of the four outputs are spent is random"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn related_second_input_is_not_added() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    for wallet in ["w_same_tx", "w_adjacent_block"] {
        probe
            .expect_selection(wallet, XMR, vec![(3 * XMR, 0)])
            .await;
    }

    let outputs = probe.scan("w_same_tx").await;
    let prepared = prepare_send_inputs_with_rpc(
        &probe.rpc,
        &outputs,
        probe.height().await,
        XMR,
        1,
        None,
        None,
        0,
        FeePolicy::default(),
    )
    .await
    .expect("select");
    let mut wallet_state = monero_rust::WalletState::new();
    wallet_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    wallet_state.add_outputs(outputs.clone());
    let built = create_transaction_with_rpc(
        &probe.rpc,
        &probe.seed("w_same_tx"),
        "mainnet",
        prepared.stored_outputs,
        &[(probe.recipient.clone(), XMR)],
        "",
        &mut wallet_state,
    )
    .await
    .expect("build");
    let blob = hex::decode(&built.tx_blob).expect("hex");
    let tx = Transaction::<NotPruned>::read(&mut blob.as_slice()).expect("transaction");
    assert_eq!(tx.prefix().inputs.len(), 1);
    assert_eq!(tx.prefix().outputs.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn dust_is_neither_selected_nor_swept() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    probe
        .expect_selection("w_dust", XMR, vec![(3 * XMR, 0)])
        .await;

    let outputs = probe.scan("w_sweep_dust").await;
    for keys in probe.sweeps(&outputs).await {
        assert_eq!(describe(&outputs, &keys), vec![(XMR, 0)]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn second_input_is_the_smallest_and_large_outputs_are_kept() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    probe
        .expect_selection("w_unrelated", XMR, vec![(XMR / 100, 0), (3 * XMR, 0)])
        .await;
    probe
        .expect_selection("w_reservation", XMR, vec![(3 * XMR, 0)])
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_spends_one_subaddress() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    let outputs = probe.scan("w_sweep").await;
    let mut swept = BTreeSet::new();
    for keys in probe.sweeps(&outputs).await {
        swept.insert(describe(&outputs, &keys));
    }
    assert_eq!(
        swept,
        BTreeSet::from([vec![(XMR, 3)], vec![(XMR, 7)]]),
        "one subaddress at random, the primary address last"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn single_preferred_input_covers_a_two_input_fee() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    probe
        .expect_selection(
            "w_single_threshold",
            XMR - 1850 * 1_200_000,
            vec![(10 * XMR, 0)],
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_outputs_of_one_transaction_are_not_preferred() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    probe
        .expect_selection(
            "w_same_tx_pair",
            3 * XMR / 2,
            vec![(XMR / 10, 0), (XMR, 0), (XMR, 0)],
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_of_many_outputs_is_split() {
    let Some(probe) = Probe::open().await else {
        return;
    };
    let outputs = probe.scan_range("w_many", "many_from", "many_to").await;
    assert_eq!(outputs.len(), 165);
    let height = probe.height().await;

    let one = prepare_sweep_inputs_with_rpc(
        &probe.rpc,
        &outputs,
        height,
        None,
        None,
        0,
        FeePolicy::default(),
    )
    .await;
    assert!(one.is_err());

    let mut transactions = prepare_sweep_transactions_with_rpc(
        &probe.rpc,
        &outputs,
        height,
        SweepSubaddresses::OneAtRandom,
        None,
        0,
        FeePolicy::default(),
    )
    .await
    .expect("prepare sweep");
    let inputs: Vec<usize> = transactions
        .iter()
        .map(|transaction| transaction.stored_outputs.len())
        .collect();
    assert_eq!(inputs, [146, 19]);

    let last = transactions.pop().expect("two transactions");
    let mut wallet_state = monero_rust::WalletState::new();
    wallet_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    wallet_state.add_outputs(outputs.clone());
    let built = sweep_all_with_rpc(
        &probe.rpc,
        &probe.seed("w_many"),
        "mainnet",
        last.stored_outputs,
        &probe.recipient,
        "",
        &mut wallet_state,
    )
    .await
    .expect("build");
    let blob = hex::decode(&built.tx_blob).expect("hex");
    let tx = Transaction::<NotPruned>::read(&mut blob.as_slice()).expect("transaction");
    assert_eq!(tx.prefix().inputs.len(), 19);
    assert_eq!(tx.prefix().outputs.len(), 2);
}
