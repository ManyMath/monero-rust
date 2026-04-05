// Regression test for compute_block_id against a real stagenet block
// (height 1384526, the recorded honked_bagpipe vector). The miner tx is v2,
// so the miner tx hash must use Monero's three-part transaction hash, not a
// plain keccak of the serialization.

use monero_rust::monero_backend::block::Block;
use monero_rust::scanner::compute_block_id;

#[test]
fn compute_block_id_matches_daemon_hash() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("vectors/honked_bagpipe_rpc.json")).unwrap();

    let recorded = vectors
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["route"] == "json_rpc")
        .expect("get_block vector present");
    let response: serde_json::Value =
        serde_json::from_str(recorded["response"].as_str().unwrap()).unwrap();
    let result = &response["result"];

    let blob = hex::decode(result["blob"].as_str().unwrap()).unwrap();
    let block = Block::read::<&[u8]>(&mut blob.as_ref()).unwrap();

    let expected = result["block_header"]["hash"].as_str().unwrap();
    let expected_miner_tx_hash = result["miner_tx_hash"].as_str().unwrap();

    assert_eq!(
        hex::encode(block.miner_tx.hash()),
        expected_miner_tx_hash,
        "miner tx hash should match the daemon's miner_tx_hash"
    );
    assert_eq!(
        hex::encode(compute_block_id(&block)),
        expected,
        "computed block ID should match the daemon's block hash"
    );
}
