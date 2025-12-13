import 'package:monero_extension/src/bindings/bindings.dart';

class TestHelpers {
  static OwnedOutput createMockOutput({
    required String txHash,
    required int outputIndex,
    required String amountXmr,
    required int blockHeight,
    bool spent = false,
    String? keyImage,
    (int, int)? subaddressIndex,
    String? paymentId,
    bool isCoinbase = false,
  }) {
    final amount = (double.parse(amountXmr) * 1e12).toInt();

    return OwnedOutput(
      txHash: txHash,
      outputIndex: outputIndex,
      amount: amount,
      amountXmr: amountXmr,
      key: 'mock_key_$txHash$outputIndex',
      keyOffset: 'mock_offset_$txHash$outputIndex',
      commitmentMask: 'mock_mask_$txHash$outputIndex',
      subaddressIndex: subaddressIndex,
      paymentId: paymentId,
      receivedOutputBytes: 'mock_bytes_$txHash$outputIndex',
      blockHeight: blockHeight,
      spent: spent,
      keyImage: keyImage ?? 'keyimage_$txHash$outputIndex',
      isCoinbase: isCoinbase,
      frozen: false,
    );
  }

  static BlockScanResponse createMockScanResponse({
    required int blockHeight,
    required int blockTimestamp,
    List<OwnedOutput>? outputs,
    List<String>? spentKeyImages,
    List<String>? spentKeyImageTxHashes,
    bool success = true,
    String? error,
    String blockHash = 'mock_block_hash',
    int txCount = 0,
    int daemonHeight = 0,
  }) {
    return BlockScanResponse(
      success: success,
      error: error,
      blockHeight: blockHeight,
      blockHash: blockHash,
      blockTimestamp: blockTimestamp,
      txCount: txCount,
      outputs: outputs ?? [],
      daemonHeight: daemonHeight > 0 ? daemonHeight : blockHeight,
      spentKeyImages: spentKeyImages ?? [],
      spentKeyImageTxHashes: spentKeyImageTxHashes ?? [],
    );
  }
}
