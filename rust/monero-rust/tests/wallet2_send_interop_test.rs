//! Destination sets where wallet2 (22578c3f7) decides the shape of a transaction.

mod common;

use common::offline::{unsigned_for, Wallet, FEE, INPUT_AMOUNT, PAYMENT_AMOUNT};
use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_oxide::transaction::{NotPruned, Pruned, Transaction};
use monero_rust::scanner::scan_single_transaction;
use monero_rust::unsigned_tx::{
    sign_offline, SignedTransaction, UnsignedChange, UnsignedTransaction,
};
use monero_wallet::{
    address::{AddressType, MoneroAddress, Network, SubaddressIndex},
    extra::{Extra, PaymentId},
    Scanner,
};
use zeroize::Zeroizing;

const NETWORK: Network = Network::Stagenet;
const PAYMENT_ID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

fn sender() -> Wallet {
    Wallet::from_spend(Scalar::from(2_345_678u64))
}

fn merchant() -> Wallet {
    Wallet::from_spend(Scalar::from(777u64))
}

fn other() -> Wallet {
    Wallet::from_spend(Scalar::from(778u64))
}

fn integrated(wallet: &Wallet) -> MoneroAddress {
    let standard = wallet.address(NETWORK);
    MoneroAddress::new(
        NETWORK,
        AddressType::LegacyIntegrated(PAYMENT_ID),
        standard.spend(),
        standard.view(),
    )
}

fn sign(wallet: &Wallet, unsigned: &UnsignedTransaction) -> Result<SignedTransaction, String> {
    sign_offline(
        &mut rand::rngs::OsRng,
        &Zeroizing::new(wallet.spend),
        &Zeroizing::new(wallet.view),
        unsigned,
    )
}

fn extra_of(tx: &Transaction<NotPruned>) -> Extra {
    Extra::read(&mut tx.prefix().extra.as_slice()).expect("transaction extra")
}

fn key_counts(tx: &Transaction<NotPruned>) -> (usize, usize) {
    let (keys, additional) = extra_of(tx).keys().expect("transaction public key");
    (keys.len(), additional.map_or(0, |keys| keys.len()))
}

fn transaction_public_key(tx: &Transaction<NotPruned>) -> EdwardsPoint {
    let (keys, _) = extra_of(tx).keys().expect("transaction public key");
    keys[0].into()
}

fn received(
    wallet: &Wallet,
    subaddresses: &[(u32, u32)],
    tx: &Transaction<NotPruned>,
) -> Vec<(Option<(u32, u32)>, u64, Option<PaymentId>)> {
    let mut scanner = Scanner::new(wallet.view_pair());
    for (account, index) in subaddresses {
        scanner.register_subaddress(SubaddressIndex::new(*account, *index).expect("subaddress"));
    }
    let pruned = Transaction::<Pruned>::from(tx.clone());
    let mut found: Vec<_> = scan_single_transaction(&mut scanner, tx.hash(), &pruned)
        .expect("scan signed transaction")
        .into_iter()
        .map(|output| {
            (
                output
                    .subaddress()
                    .map(|index| (index.account(), index.address())),
                output.commitment().amount,
                output.payment_id(),
            )
        })
        .collect();
    found.sort_by_key(|(_, amount, _)| *amount);
    found
}

fn amounts(
    found: &[(Option<(u32, u32)>, u64, Option<PaymentId>)],
) -> Vec<(Option<(u32, u32)>, u64)> {
    found
        .iter()
        .map(|(subaddress, amount, _)| (*subaddress, *amount))
        .collect()
}

#[test]
#[ignore = "known defect: an integrated address is accepted together with other destinations, which wallet2 refuses to build"]
fn integrated_address_with_another_destination_is_refused() {
    let sender = sender();
    for second in [other().address(NETWORK), other().subaddress(NETWORK, 0, 1)] {
        let unsigned = unsigned_for(
            &sender,
            NETWORK,
            vec![
                (integrated(&merchant()), PAYMENT_AMOUNT / 2),
                (second, PAYMENT_AMOUNT / 2),
            ],
        );
        let preview = unsigned.resolve(&sender.view_pair());
        assert!(preview.is_err(), "a preview shows a fee for {second}");
        let error = sign(&sender, &unsigned)
            .err()
            .unwrap_or_else(|| panic!("signed an integrated address together with {second}"));
        assert!(error.contains("payment ID"), "{error}");
    }
}

#[test]
fn integrated_address_with_the_same_keys_or_change_is_signed() {
    let sender = sender();
    let merchant = merchant();

    let alone = unsigned_for(
        &sender,
        NETWORK,
        vec![(integrated(&merchant), PAYMENT_AMOUNT)],
    );
    let signed = sign(&sender, &alone).expect("integrated address and change");
    assert_eq!(
        received(&merchant, &[], &signed.tx),
        vec![(None, PAYMENT_AMOUNT, Some(PaymentId::Encrypted(PAYMENT_ID)))]
    );

    let twice = unsigned_for(
        &sender,
        NETWORK,
        vec![
            (integrated(&merchant), PAYMENT_AMOUNT / 4),
            (merchant.address(NETWORK), PAYMENT_AMOUNT / 2),
        ],
    );
    let signed = sign(&sender, &twice).expect("integrated and standard form of one wallet");
    assert_eq!(
        received(&merchant, &[], &signed.tx),
        vec![
            (
                None,
                PAYMENT_AMOUNT / 4,
                Some(PaymentId::Encrypted(PAYMENT_ID))
            ),
            (
                None,
                PAYMENT_AMOUNT / 2,
                Some(PaymentId::Encrypted(PAYMENT_ID))
            ),
        ]
    );

    let with_self = unsigned_for(
        &sender,
        NETWORK,
        vec![
            (integrated(&merchant), PAYMENT_AMOUNT / 4),
            (sender.address(NETWORK), PAYMENT_AMOUNT / 2),
        ],
    );
    let signed = sign(&sender, &with_self).expect("integrated address and the change address");
    assert_eq!(
        received(&merchant, &[], &signed.tx),
        vec![(
            None,
            PAYMENT_AMOUNT / 4,
            Some(PaymentId::Encrypted(PAYMENT_ID))
        )]
    );
}

#[test]
fn one_subaddress_destination_uses_a_single_transaction_key() {
    let sender = sender();
    let recipient = merchant();
    let subaddress = recipient.subaddress(NETWORK, 0, 1);
    let subaddress_spend: EdwardsPoint = subaddress.spend().into();
    let change = INPUT_AMOUNT - PAYMENT_AMOUNT - FEE;
    let (first, second) = (PAYMENT_AMOUNT / 4, PAYMENT_AMOUNT - PAYMENT_AMOUNT / 4);

    let unsigned = unsigned_for(
        &sender,
        NETWORK,
        vec![(subaddress, first), (subaddress, second)],
    );
    let signed = sign(&sender, &unsigned).expect("same subaddress twice");
    assert_eq!(signed.tx.prefix().outputs.len(), 3);
    assert_eq!(key_counts(&signed.tx), (1, 0));
    assert_eq!(signed.tx.prefix().extra.len(), 33);
    assert!(signed.additional_tx_keys.is_empty());
    assert_eq!(
        transaction_public_key(&signed.tx),
        *signed.tx_key * subaddress_spend
    );
    assert_eq!(
        amounts(&received(&recipient, &[(0, 1)], &signed.tx)),
        vec![(Some((0, 1)), first), (Some((0, 1)), second)]
    );
    assert_eq!(
        amounts(&received(&sender, &[], &signed.tx)),
        vec![(None, change)]
    );

    let unsigned = unsigned_for(
        &sender,
        NETWORK,
        vec![(subaddress, first), (sender.address(NETWORK), second)],
    );
    let signed = sign(&sender, &unsigned).expect("subaddress and own change address");
    assert_eq!(signed.tx.prefix().outputs.len(), 3);
    assert_eq!(key_counts(&signed.tx), (1, 0));
    assert_eq!(signed.tx.prefix().extra.len(), 33);
    assert_eq!(
        transaction_public_key(&signed.tx),
        *signed.tx_key * subaddress_spend
    );
    assert_eq!(
        amounts(&received(&recipient, &[(0, 1)], &signed.tx)),
        vec![(Some((0, 1)), first)]
    );
    assert_eq!(
        amounts(&received(&sender, &[], &signed.tx)),
        vec![(None, change), (None, second)]
    );

    let mut unsigned = unsigned_for(
        &sender,
        NETWORK,
        vec![
            (subaddress, first),
            (subaddress, INPUT_AMOUNT - FEE - first),
        ],
    );
    unsigned.change = None;
    let signed = sign(&sender, &unsigned).expect("same subaddress twice without change");
    assert_eq!(signed.tx.prefix().outputs.len(), 2);
    assert_eq!(key_counts(&signed.tx), (1, 0));
    assert_eq!(signed.tx.prefix().extra.len(), 44);
    assert_eq!(
        transaction_public_key(&signed.tx),
        *signed.tx_key * subaddress_spend
    );
    assert_eq!(
        amounts(&received(&recipient, &[(0, 1)], &signed.tx)),
        vec![
            (Some((0, 1)), first),
            (Some((0, 1)), INPUT_AMOUNT - FEE - first)
        ]
    );
}

#[test]
fn payment_to_the_change_subaddress_is_not_a_subaddress_destination() {
    let sender = sender();
    let recipient = other();
    let own_change = sender.subaddress(NETWORK, 1, 0);
    let change = INPUT_AMOUNT - PAYMENT_AMOUNT - FEE;
    let (to_self, to_other) = (PAYMENT_AMOUNT / 4, PAYMENT_AMOUNT - PAYMENT_AMOUNT / 4);

    let mut unsigned = unsigned_for(
        &sender,
        NETWORK,
        vec![
            (own_change, to_self),
            (recipient.address(NETWORK), to_other),
        ],
    );
    unsigned.source_account = 1;
    unsigned.change = Some(UnsignedChange::Own {
        address: own_change,
        subaddress: SubaddressIndex::new(1, 0),
    });
    let signed = sign(&sender, &unsigned).expect("own change subaddress and a standard address");

    assert_eq!(signed.tx.prefix().outputs.len(), 3);
    assert_eq!(key_counts(&signed.tx), (1, 0));
    assert_eq!(signed.tx.prefix().extra.len(), 33);
    assert_eq!(
        transaction_public_key(&signed.tx),
        &*signed.tx_key * ED25519_BASEPOINT_TABLE
    );
    assert_eq!(
        amounts(&received(&recipient, &[], &signed.tx)),
        vec![(None, to_other)]
    );
    assert_eq!(
        amounts(&received(&sender, &[(1, 0)], &signed.tx)),
        vec![(Some((1, 0)), change), (Some((1, 0)), to_self)]
    );
}

#[test]
#[ignore = "known defect: two different destinations without change get a dummy payment ID, where wallet2 writes none"]
fn dummy_payment_id_needs_a_single_destination() {
    let sender = sender();
    let (first, second) = (merchant(), other());
    let half = (INPUT_AMOUNT - FEE) / 2;
    let rest = INPUT_AMOUNT - FEE - half;
    let without_change = |payments| {
        let mut unsigned = unsigned_for(&sender, NETWORK, payments);
        unsigned.change = None;
        sign(&sender, &unsigned).expect("two destinations without change")
    };

    let signed = without_change(vec![
        (first.address(NETWORK), half),
        (second.address(NETWORK), rest),
    ]);
    assert_eq!(signed.tx.prefix().outputs.len(), 2);
    assert_eq!(key_counts(&signed.tx), (1, 0));
    assert!(extra_of(&signed.tx).payment_id().is_none());
    assert_eq!(signed.tx.prefix().extra.len(), 33);
    assert_eq!(received(&first, &[], &signed.tx), vec![(None, half, None)]);
    assert_eq!(received(&second, &[], &signed.tx), vec![(None, rest, None)]);

    let signed = without_change(vec![
        (first.subaddress(NETWORK, 0, 1), half),
        (second.address(NETWORK), rest),
    ]);
    assert_eq!(key_counts(&signed.tx), (1, 2));
    assert!(extra_of(&signed.tx).payment_id().is_none());
    assert_eq!(signed.tx.prefix().extra.len(), 33 + 2 + 64);
    assert_eq!(
        received(&first, &[(0, 1)], &signed.tx),
        vec![(Some((0, 1)), half, None)]
    );
    assert_eq!(received(&second, &[], &signed.tx), vec![(None, rest, None)]);

    let signed = without_change(vec![
        (first.address(NETWORK), half),
        (first.address(NETWORK), rest),
    ]);
    assert_eq!(signed.tx.prefix().extra.len(), 44);
    let zero = Some(PaymentId::Encrypted([0; 8]));
    let mut expected = vec![(None, half, zero), (None, rest, zero)];
    expected.sort_by_key(|(_, amount, _)| *amount);
    assert_eq!(received(&first, &[], &signed.tx), expected);
}

mod regtest {
    use super::common;

    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
    use monero_oxide::transaction::{NotPruned, Pruned, Transaction};
    use monero_rust::monero_rpc::{HttpRpc, Rpc};
    use monero_rust::native::{
        broadcast_raw_transaction_with_rpc, create_transaction_with_rpc, StoredOutputData,
        TransactionResult,
    };
    use monero_rust::scanner::{
        derive_address, scan_blocks_batch, scan_single_transaction, Lookahead,
    };
    use zeroize::Zeroizing;

    const SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
        sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
        fences goat unquoted";
    const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;
    const MIN_HEIGHT_TO_SPEND: usize = 750;
    const LOOKAHEAD: Lookahead = Lookahead {
        account: 0,
        subaddress: 5,
    };

    fn recipient(spend: u64, view: u64) -> monero_wallet::ViewPair {
        monero_wallet::ViewPair::new(
            monero_wallet::ed25519::Point::from(&Scalar::from(spend) * ED25519_BASEPOINT_TABLE),
            Zeroizing::new(monero_wallet::ed25519::Scalar::from(Scalar::from(view))),
        )
        .expect("recipient view pair")
    }

    fn parse(result: &TransactionResult) -> Transaction<NotPruned> {
        let bytes = hex::decode(&result.tx_blob).expect("signed transaction hex");
        Transaction::<NotPruned>::read(&mut bytes.as_slice()).expect("signed transaction")
    }

    fn paid_to(pair: &monero_wallet::ViewPair, tx: &Transaction<NotPruned>) -> Vec<u64> {
        let mut scanner = monero_wallet::Scanner::new(pair.clone());
        scan_single_transaction(
            &mut scanner,
            tx.hash(),
            &Transaction::<Pruned>::from(tx.clone()),
        )
        .expect("scan recipient")
        .into_iter()
        .map(|output| output.commitment().amount)
        .collect()
    }

    async fn mature_outputs(rpc: &Rpc<HttpRpc>) -> Vec<StoredOutputData> {
        let address = derive_address(SEED, NETWORK, "")
            .expect("derive address")
            .to_string();
        let height = rpc.get_height().await.expect("get_height");
        if height < MIN_HEIGHT_TO_SPEND {
            rpc.generate_blocks(&address, MIN_HEIGHT_TO_SPEND - height)
                .await
                .expect("mine enough decoys");
        }
        let start = rpc.get_height().await.expect("get_height");
        rpc.generate_blocks(&address, 3 + 60)
            .await
            .expect("mine and mature three outputs");
        scan_blocks_batch(rpc, start as u64, SEED, NETWORK, LOOKAHEAD, false, "")
            .await
            .expect("scan mined blocks")
            .into_iter()
            .filter(|block| block.block_height < (start + 3) as u64)
            .flat_map(|block| block.outputs)
            .map(StoredOutputData::from)
            .collect()
    }

    async fn with_ring(
        rpc: &Rpc<HttpRpc>,
        mut input: StoredOutputData,
        recipients: &[(String, u64)],
    ) -> (StoredOutputData, u64) {
        let first =
            create_transaction_with_rpc(rpc, SEED, "mainnet", vec![input.clone()], recipients, "")
                .await
                .expect("first build");
        input.ring = Some(first.input_rings[0].ring.clone());
        (input, first.fee)
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "known defect: a transfer is refused unless its inputs also cover an upper bound on the fee, so it cannot use them exactly as wallet2 can"]
    async fn transfer_can_use_its_inputs_exactly() {
        let Some(node) = common::regtest_node() else {
            return;
        };
        let rpc = HttpRpc::new(node).expect("regtest RPC");
        let mut inputs = mature_outputs(&rpc).await;
        assert_eq!(inputs.len(), 3, "three matured outputs of the test wallet");
        let (first, second) = (recipient(2777, 2888), recipient(3777, 3888));
        let first_address = first.legacy_address(NETWORK).to_string();
        let second_address = second.legacy_address(NETWORK).to_string();
        let small = 1_000_000_000u64;

        let (input, fee) = with_ring(
            &rpc,
            inputs.pop().unwrap(),
            &[(first_address.clone(), small)],
        )
        .await;
        let amount = input.amount - fee;

        let short = create_transaction_with_rpc(
            &rpc,
            SEED,
            "mainnet",
            vec![input.clone()],
            &[(first_address.clone(), amount + 1)],
            "",
        )
        .await
        .expect_err("the inputs do not cover the fee");
        assert!(short.starts_with("Insufficient funds"), "{short}");

        let with_change = create_transaction_with_rpc(
            &rpc,
            SEED,
            "mainnet",
            vec![input.clone()],
            &[(first_address.clone(), amount - 1)],
            "",
        )
        .await
        .expect("one piconero of change");
        assert_eq!(with_change.fee, fee);
        assert_eq!(with_change.change_outputs.len(), 1);
        assert_eq!(with_change.change_outputs[0].amount, 1);

        let exact = create_transaction_with_rpc(
            &rpc,
            SEED,
            "mainnet",
            vec![input],
            &[(first_address.clone(), amount)],
            "",
        )
        .await
        .expect("a transfer that uses its input exactly");
        let tx = parse(&exact);
        assert_eq!(exact.fee, fee);
        assert!(
            exact.change_outputs.is_empty(),
            "the second output is a dummy"
        );
        assert_eq!(tx.prefix().outputs.len(), 2);
        assert_eq!(tx.prefix().extra.len(), 44);
        assert_eq!(paid_to(&first, &tx), vec![amount]);
        broadcast_raw_transaction_with_rpc(&rpc, &exact.tx_blob, false)
            .await
            .expect("daemon accepts the zero-change transfer");

        let (input, fee) = with_ring(
            &rpc,
            inputs.pop().unwrap(),
            &[
                (first_address.clone(), small),
                (second_address.clone(), small),
            ],
        )
        .await;
        let rest = input.amount - small - fee;
        let exact = create_transaction_with_rpc(
            &rpc,
            SEED,
            "mainnet",
            vec![input],
            &[(first_address, small), (second_address, rest)],
            "",
        )
        .await
        .expect("two destinations that use the input exactly");
        let tx = parse(&exact);
        assert_eq!(exact.fee, fee);
        assert!(exact.change_outputs.is_empty());
        assert_eq!(tx.prefix().outputs.len(), 2);
        assert_eq!(tx.prefix().extra.len(), 33);
        assert_eq!(paid_to(&first, &tx), vec![small]);
        assert_eq!(paid_to(&second, &tx), vec![rest]);
        broadcast_raw_transaction_with_rpc(&rpc, &exact.tx_blob, false)
            .await
            .expect("daemon accepts the transfer without change");
    }
}
