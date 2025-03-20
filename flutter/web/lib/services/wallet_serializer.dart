import 'package:tuple/tuple.dart';
import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';

/// Pure serialization/deserialization for wallet data.
/// No dart:html dependency — safe to import in tests.
class WalletSerializer {
  /// Current wallet data format version.
  /// - 1: Initial version with accounts, scanningAccounts, and all fields required
  static const int currentVersion = 1;

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
    required List<int> accounts,
    required Map<int, List<OwnedOutput>> outputsByAccount,
    required int activeAccount,
    required Set<int> scanningAccounts,
  }) {
    return {
      'version': currentVersion,
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
                'isCoinbase': o.isCoinbase,
              })
          .toList(),
      'transactions': transactions.map((t) => t.toJson()).toList(),
      'scanState': {
        'continuousScanCurrentHeight': continuousScanCurrentHeight,
      },
      'selectedOutputs': selectedOutputs.toList(),
      'accounts': accounts,
      'activeAccount': activeAccount,
      'scanningAccounts': scanningAccounts.toList(),
      'outputsByAccount': outputsByAccount.map((accountIndex, outputs) {
          return MapEntry(
            accountIndex.toString(),
            outputs.map((o) => {
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
              'isCoinbase': o.isCoinbase,
            }).toList(),
          );
        }),
    };
  }

  /// Deserialize wallet data from JSON map.
  /// This is the canonical parsing logic used by WalletPersistenceService.
  ///
  /// Backward Compatibility Note:
  /// The 'spent' and 'isCoinbase' fields were added to outputs in later versions.
  /// To support loading older wallet files that lack these fields, we use a
  /// consistent pattern across all deserialization code:
  /// 1. Check if the field exists in the JSON (containsKey)
  /// 2. Check if the value is not null
  /// 3. If both checks pass, cast to the expected type
  /// 4. Otherwise, use a safe default (false for both fields)
  ///
  /// This same pattern is used in:
  /// - wallet_serializer.dart (this file)
  /// - wallet_instance.dart fromJson()
  /// - wallet_transaction.dart fromJson()
  static ({
    String seed,
    String network,
    String? address,
    String nodeUrl,
    List<OwnedOutput> outputs,
    List<WalletTransaction> transactions,
    int continuousScanCurrentHeight,
    Set<String> selectedOutputs,
    List<int> accounts,
    Map<int, List<OwnedOutput>> outputsByAccount,
    int activeAccount,
    Set<int> scanningAccounts,
  }) deserialize(Map<String, dynamic> walletData) {
    // Check version compatibility
    final version = walletData['version'] as int?;
    if (version == null) {
      throw FormatException('Wallet data missing version field');
    }
    if (version > currentVersion) {
      throw FormatException('Wallet data version $version is newer than supported version $currentVersion. Please update the application.');
    }
    // In the future, add migration logic here for older versions if needed
    // For now, we only support version 1
    if (version < currentVersion) {
      throw FormatException('Wallet data version $version is no longer supported. Please re-export your wallet.');
    }

    final outputs = (walletData['outputs'] as List).map((o) {
      final d = o as Map<String, dynamic>;

      // Handle missing fields for backward compatibility
      // These fields were added later and might not exist in older wallet data
      // Also handle cases where the field exists but has a null value
      final bool spentValue = d.containsKey('spent') && d['spent'] != null
          ? d['spent'] as bool
          : false;  // Assume unspent if field is missing or null
      final bool isCoinbaseValue = d.containsKey('isCoinbase') && d['isCoinbase'] != null
          ? d['isCoinbase'] as bool
          : false;  // Assume not coinbase if field is missing or null

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
        spent: spentValue,
        keyImage: d['keyImage'] as String,
        isCoinbase: isCoinbaseValue,
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

    // Parse account-related fields
    final accounts = (walletData['accounts'] as List).map((e) => e as int).toList();
    final activeAccount = walletData['activeAccount'] as int;
    final scanningAccounts = Set<int>.from((walletData['scanningAccounts'] as List).map((e) => e as int));

    final Map<int, List<OwnedOutput>> outputsByAccount = {};
    final outputsByAccountJson = walletData['outputsByAccount'] as Map<String, dynamic>;
    outputsByAccountJson.forEach((key, value) {
      final accountIndex = int.parse(key);
      final accountOutputs = (value as List).map((o) {
        final d = o as Map<String, dynamic>;

        // Handle missing fields for backward compatibility
        // Also handle cases where the field exists but has a null value
        final bool spentValue = d.containsKey('spent') && d['spent'] != null
            ? d['spent'] as bool
            : false;  // Assume unspent if field is missing or null
        final bool isCoinbaseValue = d.containsKey('isCoinbase') && d['isCoinbase'] != null
            ? d['isCoinbase'] as bool
            : false;  // Assume not coinbase if field is missing or null

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
          spent: spentValue,
          keyImage: d['keyImage'] as String,
          isCoinbase: isCoinbaseValue,
        );
      }).toList();
      outputsByAccount[accountIndex] = accountOutputs;
    });

    return (
      seed: walletData['seed'] as String,
      network: walletData['network'] as String,
      address: walletData['address'] as String?,
      nodeUrl: walletData['nodeUrl'] as String,
      outputs: outputs,
      transactions: transactions,
      continuousScanCurrentHeight:
          scanState['continuousScanCurrentHeight'] as int,
      selectedOutputs: selectedOutputs,
      accounts: accounts,
      outputsByAccount: outputsByAccount,
      activeAccount: activeAccount,
      scanningAccounts: scanningAccounts,
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
