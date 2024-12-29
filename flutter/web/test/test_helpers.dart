import 'package:tuple/tuple.dart';
import '../lib/src/bindings/bindings.dart';

/// Test helper utilities for creating test fixtures and mock data.
class TestHelpers {
  /// Create a mock OwnedOutput for testing
  static OwnedOutput createMockOutput({
    required String txHash,
    required int outputIndex,
    required String amountXmr,
    required int blockHeight,
    bool spent = false,
    String? keyImage,
    Tuple2<int, int>? subaddressIndex,
    String? paymentId,
  }) {
    // Convert XMR string to atomic units (1 XMR = 1e12 atomic units)
    final amount = (double.parse(amountXmr) * 1e12).toInt();

    return OwnedOutput(
      txHash: txHash,
      outputIndex: outputIndex,
      amount: Uint64(BigInt.from(amount)),
      amountXmr: amountXmr,
      key: 'mock_key_$txHash$outputIndex',
      keyOffset: 'mock_offset_$txHash$outputIndex',
      commitmentMask: 'mock_mask_$txHash$outputIndex',
      subaddressIndex: subaddressIndex,
      paymentId: paymentId,
      receivedOutputBytes: 'mock_bytes_$txHash$outputIndex',
      blockHeight: Uint64(BigInt.from(blockHeight)),
      spent: spent,
      keyImage: keyImage ?? 'keyimage_$txHash$outputIndex',
    );
  }

  /// Create a mock BlockScanResponse for testing
  static BlockScanResponse createMockScanResponse({
    required int blockHeight,
    required int blockTimestamp,
    List<OwnedOutput>? outputs,
    List<String>? spentKeyImages,
    bool success = true,
    String? error,
    String blockHash = 'mock_block_hash',
    int txCount = 0,
    int daemonHeight = 0,
  }) {
    return BlockScanResponse(
      success: success,
      error: error,
      blockHeight: Uint64(BigInt.from(blockHeight)),
      blockHash: blockHash,
      blockTimestamp: Uint64(BigInt.from(blockTimestamp)),
      txCount: txCount,
      outputs: outputs ?? [],
      daemonHeight: Uint64(BigInt.from(daemonHeight > 0 ? daemonHeight : blockHeight)),
      spentKeyImages: spentKeyImages ?? [],
    );
  }
}
