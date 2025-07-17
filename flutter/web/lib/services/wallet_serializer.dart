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
    required int activeAccount,
    required Set<int> scanningAccounts,
    String? blockHashesJson,
    String? pendingStateJson,
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
                    ? [o.subaddressIndex![0], o.subaddressIndex![1]]
                    : null,
                'paymentId': o.paymentId,
                'receivedOutputBytes': o.receivedOutputBytes,
                'blockHeight': o.blockHeight.toString(),
                'spent': o.spent,
                'keyImage': o.keyImage,
                'isCoinbase': o.isCoinbase,
                'frozen': o.frozen,
              })
          .toList(),
      'transactions': transactions.map((t) => {
            'txHash': t.txHash,
            'blockHeight': t.blockHeight,
            'blockTimestamp': t.blockTimestamp,
            'receivedOutputRefs': t.receivedOutputs
                .map((o) => '${o.txHash}:${o.outputIndex}')
                .toList(),
            'spentKeyImages': t.spentKeyImages,
            if (t.description != null) 'description': t.description,
          }).toList(),
      'scanState': {
        'continuousScanCurrentHeight': continuousScanCurrentHeight,
      },
      'selectedOutputs': selectedOutputs.toList(),
      'accounts': accounts,
      'activeAccount': activeAccount,
      'scanningAccounts': scanningAccounts.toList(),
      if (blockHashesJson != null) 'blockHashesJson': blockHashesJson,
      if (pendingStateJson != null) 'pendingStateJson': pendingStateJson,
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
    String? blockHashesJson,
    String? pendingStateJson,
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
        amount: int.parse(d['amount'] as String),
        amountXmr: d['amountXmr'] as String,
        key: d['key'] as String,
        keyOffset: d['keyOffset'] as String,
        commitmentMask: d['commitmentMask'] as String,
        subaddressIndex: d['subaddressIndex'] != null
            ? [
                d['subaddressIndex'][0] as int,
                d['subaddressIndex'][1] as int,
              ]
            : null,
        paymentId: d['paymentId'] as String?,
        receivedOutputBytes: d['receivedOutputBytes'] as String,
        blockHeight: int.parse(d['blockHeight'] as String),
        spent: spentValue,
        keyImage: d['keyImage'] as String,
        isCoinbase: isCoinbaseValue,
        frozen: d.containsKey('frozen') && d['frozen'] != null
            ? d['frozen'] as bool
            : false,
      );
    }).toList();

    final Map<String, OwnedOutput> outputLookup = {
      for (var o in outputs) '${o.txHash}:${o.outputIndex}': o
    };

    final transactions = walletData['transactions'] != null
        ? (walletData['transactions'] as List).map((t) {
            final txData = t as Map<String, dynamic>;
            if (txData.containsKey('receivedOutputRefs')) {
              return WalletTransaction.fromJsonCompact(txData, outputLookup);
            }
            return WalletTransaction.fromJson(txData);
          }).toList()
        : <WalletTransaction>[];

    final scanState = walletData['scanState'] as Map<String, dynamic>;
    final selectedOutputs =
        Set<String>.from(walletData['selectedOutputs'] as List);

    // Parse account-related fields
    final accounts = (walletData['accounts'] as List).map((e) => e as int).toList();
    final activeAccount = walletData['activeAccount'] as int;
    final scanningAccounts = Set<int>.from((walletData['scanningAccounts'] as List).map((e) => e as int));

    // Derive outputsByAccount from the flat outputs list (no longer stored separately)
    final Map<int, List<OwnedOutput>> outputsByAccount = {};
    for (var output in outputs) {
      final account = output.subaddressIndex?[0] ?? 0;
      outputsByAccount.putIfAbsent(account, () => []).add(output);
    }

    final blockHashesJson = walletData['blockHashesJson'] as String?;
    final pendingStateJson = walletData['pendingStateJson'] as String?;

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
      blockHashesJson: blockHashesJson,
      pendingStateJson: pendingStateJson,
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
