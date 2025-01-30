import 'package:tuple/tuple.dart';
import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';

/// Pure serialization/deserialization for wallet data.
/// No dart:html dependency — safe to import in tests.
class WalletSerializer {
  /// Serialize wallet state to JSON map.
  /// This is the canonical format used by WalletPersistenceService.
  static Map<String, dynamic> serialize({
    required String seed,
    required String network,
    required String? address,
    required String nodeUrl,
    required List<OwnedOutput> outputs,
    required List<WalletTransaction> transactions,
    required int continuousScanCurrentHeight,
    required Set<String> selectedOutputs,
  }) {
    return {
      'seed': seed,
      'network': network,
      'address': address,
      'nodeUrl': nodeUrl,
      'outputs': outputs
          .map((o) => {
                'txHash': o.txHash,
                'outputIndex': o.outputIndex,
                'amount': o.amount.toString(),
                'amountXmr': o.amountXmr,
                'key': o.key,
                'keyOffset': o.keyOffset,
                'commitmentMask': o.commitmentMask,
                'subaddressIndex': o.subaddressIndex != null
                    ? [o.subaddressIndex!.item1, o.subaddressIndex!.item2]
                    : null,
                'paymentId': o.paymentId,
                'receivedOutputBytes': o.receivedOutputBytes,
                'blockHeight': o.blockHeight.toString(),
                'spent': o.spent,
                'keyImage': o.keyImage,
              })
          .toList(),
      'transactions': transactions.map((t) => t.toJson()).toList(),
      'scanState': {
        'continuousScanCurrentHeight': continuousScanCurrentHeight,
      },
      'selectedOutputs': selectedOutputs.toList(),
    };
  }

  /// Deserialize wallet data from JSON map.
  /// This is the canonical parsing logic used by WalletPersistenceService.
  static ({
    String seed,
    String network,
    String? address,
    String nodeUrl,
    List<OwnedOutput> outputs,
    List<WalletTransaction> transactions,
    int continuousScanCurrentHeight,
    Set<String> selectedOutputs,
  }) deserialize(Map<String, dynamic> walletData) {
    final outputs = (walletData['outputs'] as List).map((o) {
      final d = o as Map<String, dynamic>;
      return OwnedOutput(
        txHash: d['txHash'] as String,
        outputIndex: d['outputIndex'] as int,
        amount: Uint64(BigInt.parse(d['amount'] as String)),
        amountXmr: d['amountXmr'] as String,
        key: d['key'] as String,
        keyOffset: d['keyOffset'] as String,
        commitmentMask: d['commitmentMask'] as String,
        subaddressIndex: d['subaddressIndex'] != null
            ? Tuple2<int, int>(
                d['subaddressIndex'][0] as int,
                d['subaddressIndex'][1] as int,
              )
            : null,
        paymentId: d['paymentId'] as String?,
        receivedOutputBytes: d['receivedOutputBytes'] as String,
        blockHeight: Uint64(BigInt.parse(d['blockHeight'] as String)),
        spent: d['spent'] as bool,
        keyImage: d['keyImage'] as String,
        isCoinbase: (d['isCoinbase'] as bool?) ?? false,
      );
    }).toList();

    final transactions = walletData['transactions'] != null
        ? (walletData['transactions'] as List)
            .map((t) => WalletTransaction.fromJson(t as Map<String, dynamic>))
            .toList()
        : <WalletTransaction>[];

    final scanState = walletData['scanState'] as Map<String, dynamic>;
    final selectedOutputs =
        Set<String>.from(walletData['selectedOutputs'] as List);

    return (
      seed: walletData['seed'] as String? ?? '',
      network: walletData['network'] as String? ?? 'stagenet',
      address: walletData['address'] as String?,
      nodeUrl: walletData['nodeUrl'] as String? ?? 'http://127.0.0.1:38081',
      outputs: outputs,
      transactions: transactions,
      continuousScanCurrentHeight:
          scanState['continuousScanCurrentHeight'] as int,
      selectedOutputs: selectedOutputs,
    );
  }

  /// Get the storage key for a wallet ID.
  static String getStorageKey(String walletId) => 'monero_wallet_$walletId';

  /// Extract suggested wallet ID from filename.
  static String extractWalletIdFromFilename(String filename) {
    String id = filename;

    if (id.endsWith('.monero-wallet')) {
      id = id.substring(0, id.length - 14);
    }

    final timestampRegex = RegExp(r'_\d{8}-\d{6}$');
    id = id.replaceAll(timestampRegex, '');

    if (id.isEmpty || !RegExp(r'^[a-zA-Z0-9_-]+$').hasMatch(id)) {
      id = 'imported_wallet';
    }

    return id;
  }
}
