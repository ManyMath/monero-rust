use zeroize::Zeroizing;

#[cfg(feature = "compile-time-generators")]
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
#[cfg(not(feature = "compile-time-generators"))]
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT as ED25519_BASEPOINT_TABLE;

use crate::{
  ed25519::*,
  ringct::EncryptedAmount,
  transaction::{Timelock, Pruned, Transaction},
  block::Block,
  interface::ScannableBlock,
  PaymentId,
  output::{AbsoluteId, RelativeId, OutputData, Metadata},
  WalletOutput, ViewPair, Scanner,
};

const SPEND_KEY: &str = "ccf0ea10e1ea64354f42fa710c2b318e581969cf49046d809d1f0aadb3fc7a02";
const VIEW_KEY: &str = "a28b4b2085592881df94ee95da332c16b5bb773eb8bb74730208cbb236c73806";

#[rustfmt::skip]
const PRUNED_TX_WITH_LONG_ENCRYPTED_AMOUNT: &str = "020001020003060101cf60390bb71aa15eb24037772012d59dc68cb4b6211e1c93206db09a6c346261020002ee8ca293511571c0005e1c144e49d09b8ff03046dbafb3e064a34cb9fc1994b600029e2e5cd08c8681dbcf2ce66071467e835f7e86613fbfed3c4fb170127b94e1072c01d3ce2a622c6e06ed465f81017dd6188c3a6e3d8e65a846f9c98416da0e150a82020901c553d35e54111bd001e0bbcbf289d701ce90e309ead2b487ec1d4d8af5d649543eb99a7620f6b54e532898527be29704f050e6f06de61e5967b2ddd506b4d6d36546065d6aae156ac7bec18c99580c07867fb98cb29853edbafec91af2df605c12f9aaa81a9165625afb6649f5a652012c5ba6612351140e1fb4a8463cc765d0a9bb7d999ba35750f365c5285d77230b76c7a612784f4845812a2899f2ca6a304fee61362db59b263115c27d2ce78af6b1d9e939c1f4036c7707851f41abe6458cf1c748353e593469ebf43536a939f7";

#[rustfmt::skip]
const BLOCK: &str = "0202e8e28efe04db09e2fc4d57854786220bd33e0169ff692440d27ae3932b9219df9ab1d7260b00000000014101ff050580d0acf30e02704972eb1878e94686b62fa4c0202f3e7e3a263073bd6edd751990ea769494ee80c0fc82aa0202edac72ab7c5745d4acaa95f76a3b76e238a55743cd51efb586f968e09821788d80d0dbc3f40202f9b4cf3141aac4203a1aaed01f09326615544997d1b68964928d9aafd07e38e580a0e5b9c29101023405e3aa75b1b7adf04e8c7faa3c3d45616ae740a8b11fb7cc1555dd8b9e4c9180c0dfda8ee90602d2b78accfe1c2ae57bed4fe3385f7735a988f160ef3bbc1f9d7a0c911c26ffd92101d2d55b5066d247a97696be4a84bf70873e4f149687f57e606eb6682f11650e1701b74773bbea995079805398052da9b69244bda034b089b50e4d9151dedb59a12f";

const OUTPUT_INDEX_FOR_FIRST_RINGCT_OUTPUT: u64 = 0; // note the miner tx is a v1 tx

fn wallet_output0() -> WalletOutput {
  WalletOutput {
    absolute_id: AbsoluteId {
      transaction: hex::decode("b74773bbea995079805398052da9b69244bda034b089b50e4d9151dedb59a12f")
        .unwrap()
        .try_into()
        .unwrap(),
      index_in_transaction: 0,
    },
    relative_id: RelativeId { index_on_blockchain: OUTPUT_INDEX_FOR_FIRST_RINGCT_OUTPUT },
    data: OutputData {
      key: CompressedPoint::from(
        <[u8; 32]>::try_from(
          hex::decode("ee8ca293511571c0005e1c144e49d09b8ff03046dbafb3e064a34cb9fc1994b6").unwrap(),
        )
        .unwrap(),
      )
      .decompress()
      .unwrap(),
      key_offset: Scalar::read(
        &mut hex::decode("f1d21a76ea0bb228fbc5f0dece0597a8ffb59de7a04b29f70b7c0310446ea905")
          .unwrap()
          .as_slice(),
      )
      .unwrap(),
      commitment: Commitment {
        amount: 10000,
        mask: Scalar::read(
          &mut hex::decode("05c2f142aaf3054cbff0a022f6c7cb75403fd92af0f9441c072ade3f273f7706")
            .unwrap()
            .as_slice(),
        )
        .unwrap(),
      },
    },
    metadata: Metadata {
      additional_timelock: Timelock::None,
      subaddress: None,
      payment_id: Some(PaymentId::Encrypted([0, 0, 0, 0, 0, 0, 0, 0])),
      arbitrary_data: [].to_vec(),
    },
  }
}

fn wallet_output1() -> WalletOutput {
  WalletOutput {
    absolute_id: AbsoluteId {
      transaction: hex::decode("b74773bbea995079805398052da9b69244bda034b089b50e4d9151dedb59a12f")
        .unwrap()
        .try_into()
        .unwrap(),
      index_in_transaction: 1,
    },
    relative_id: RelativeId { index_on_blockchain: OUTPUT_INDEX_FOR_FIRST_RINGCT_OUTPUT + 1 },
    data: OutputData {
      key: CompressedPoint::from(
        <[u8; 32]>::try_from(
          hex::decode("9e2e5cd08c8681dbcf2ce66071467e835f7e86613fbfed3c4fb170127b94e107").unwrap(),
        )
        .unwrap(),
      )
      .decompress()
      .unwrap(),
      key_offset: Scalar::read(
        &mut hex::decode("c5189738c1cb40e68d464f1a1848a85f6ab2c09652a31849213dc0fefd212806")
          .unwrap()
          .as_slice(),
      )
      .unwrap(),
      commitment: Commitment {
        amount: 10000,
        mask: Scalar::read(
          &mut hex::decode("c8922ce32cb2bf454a6b77bc91423ba7a18412b71fa39a97a2a743c1fe0bad04")
            .unwrap()
            .as_slice(),
        )
        .unwrap(),
      },
    },
    metadata: Metadata {
      additional_timelock: Timelock::None,
      subaddress: None,
      payment_id: Some(PaymentId::Encrypted([0, 0, 0, 0, 0, 0, 0, 0])),
      arbitrary_data: [].to_vec(),
    },
  }
}

#[test]
fn scan_long_encrypted_amount() {
  // Parse strings
  let spend_key_buf = hex::decode(SPEND_KEY).unwrap();
  let spend_key = Zeroizing::new(Scalar::read(&mut spend_key_buf.as_slice()).unwrap());

  let view_key_buf = hex::decode(VIEW_KEY).unwrap();
  let view_key = Zeroizing::new(Scalar::read(&mut view_key_buf.as_slice()).unwrap());

  let tx_buf = hex::decode(PRUNED_TX_WITH_LONG_ENCRYPTED_AMOUNT).unwrap();
  let tx = Transaction::<Pruned>::read(&mut tx_buf.as_slice()).unwrap();

  let block_buf = hex::decode(BLOCK).unwrap();
  let block = Block::read(&mut block_buf.as_slice()).unwrap();

  // Confirm tx has long form encrypted amounts
  match &tx {
    Transaction::V2 { prefix: _, proofs } => {
      let proofs = proofs.clone().unwrap();
      assert_eq!(proofs.base.encrypted_amounts.len(), 2);
      assert!(proofs
        .base
        .encrypted_amounts
        .iter()
        .all(|o| matches!(o, EncryptedAmount::Original { .. })));
    }
    Transaction::V1 { .. } => panic!("Unexpected transaction version"),
  }

  // Prepare scanner
  let spend_pub = Point::from(&(*spend_key).into() * ED25519_BASEPOINT_TABLE);
  let view = ViewPair::new(spend_pub, view_key).unwrap();
  let mut scanner = Scanner::new(view);

  // Prepare scannable block
  let txs: Vec<Transaction<Pruned>> = vec![tx];
  let scannable_block = ScannableBlock {
    block,
    transactions: txs,
    output_index_for_first_ringct_output: Some(OUTPUT_INDEX_FOR_FIRST_RINGCT_OUTPUT),
  };

  // Scan the block
  let outputs = scanner.scan(scannable_block).unwrap().not_additionally_locked();

  assert_eq!(outputs.len(), 2);
  assert_eq!(outputs[0], wallet_output0());
  assert_eq!(outputs[1], wallet_output1());
}

// A version 1 transaction paying the view pair at its second output.
#[test]
fn scan_version_1_transaction() {
  use crate::{
    transaction::{Input, NotPruned, Output, TransactionPrefix},
    block::BlockHeader,
    Extra, SharedKeyDerivations,
  };

  let spend_key_buf = hex::decode(SPEND_KEY).unwrap();
  let spend_key = Zeroizing::new(Scalar::read(&mut spend_key_buf.as_slice()).unwrap());
  let view_key_buf = hex::decode(VIEW_KEY).unwrap();
  let view_key = Zeroizing::new(Scalar::read(&mut view_key_buf.as_slice()).unwrap());
  let spend_pub = Point::from(&(*spend_key).into() * ED25519_BASEPOINT_TABLE);
  let view_pub = Point::from(&(*view_key).into() * ED25519_BASEPOINT_TABLE);

  let r = curve25519_dalek::Scalar::from(987_654_321u64);
  let tx_key = Point::from(&r * ED25519_BASEPOINT_TABLE);
  let ecdh = Zeroizing::new(Point::from(r * view_pub.into()));
  let shared = SharedKeyDerivations::output_derivations(None, ecdh, 1).shared_key;
  let ours = Point::from(&shared.into() * ED25519_BASEPOINT_TABLE + spend_pub.into());
  let other = Point::from(&curve25519_dalek::Scalar::from(5u64) * ED25519_BASEPOINT_TABLE);

  let mut extra = vec![];
  Extra::new(tx_key.compress(), vec![]).write(&mut extra).unwrap();
  let prefix = |inputs, outputs| TransactionPrefix {
    additional_timelock: Timelock::None,
    inputs,
    outputs,
    extra: extra.clone(),
  };
  let output =
    |amount, key: Point| Output { amount: Some(amount), key: key.compress(), view_tag: None };
  let tx = Transaction::<NotPruned>::V1 {
    prefix: prefix(
      vec![Input::ToKey {
        amount: Some(8_000_000),
        key_offsets: vec![3],
        key_image: other.compress(),
      }],
      vec![output(1_000_000, other), output(7_000_000, ours)],
    ),
    signatures: vec![],
  };
  let miner = Transaction::<NotPruned>::V1 {
    prefix: prefix(vec![Input::Gen(1)], vec![output(1, other)]),
    signatures: vec![],
  };
  let header = BlockHeader {
    hardfork_version: 1,
    hardfork_signal: 0,
    timestamp: 0,
    previous: [0; 32],
    nonce: 0,
  };
  let block = Block::new(header, miner, vec![tx.hash()]).unwrap();

  let mut scanner = Scanner::new(ViewPair::new(spend_pub, view_key).unwrap());
  let outputs = scanner
    .scan(ScannableBlock {
      block,
      transactions: vec![Transaction::<Pruned>::from(tx.clone())],
      output_index_for_first_ringct_output: Some(1000),
    })
    .unwrap()
    .not_additionally_locked();

  assert_eq!(outputs.len(), 1);
  assert_eq!(outputs[0].transaction(), tx.hash());
  assert_eq!(outputs[0].index_in_transaction(), 1);
  assert_eq!(outputs[0].index_on_blockchain(), 1);
  assert_eq!(outputs[0].key(), ours);
  assert_eq!(outputs[0].commitment().amount, 7_000_000);
  assert_eq!(<[u8; 32]>::from(outputs[0].commitment().mask), curve25519_dalek::Scalar::ONE.to_bytes());
}
