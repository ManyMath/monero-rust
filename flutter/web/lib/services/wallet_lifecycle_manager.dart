import '../src/ffi/signal_types.dart';
import '../models/wallet_instance.dart';
import '../models/wallet_transaction.dart';
import '../utils/output_utils.dart';
import '../utils/transaction_utils.dart';
import 'wallet_persistence_service.dart';

enum SwitchResult {
  alreadyCurrent,
  switchedToOpen,
  needsLoad,
  reset,
}

class CloseWalletResult {
  final bool found;
  final WalletInstance? switchedTo;

  const CloseWalletResult._({required this.found, this.switchedTo});

  static CloseWalletResult notFound() =>
      const CloseWalletResult._(found: false);
  static CloseWalletResult closed({WalletInstance? switchedTo}) =>
      CloseWalletResult._(found: true, switchedTo: switchedTo);
}

class WalletLifecycleManager {
  final WalletPersistenceService _persistence;

  WalletLifecycleManager({required WalletPersistenceService persistence})
      : _persistence = persistence;

  String walletId = '';
  List<String> availableWalletIds = [];
  final Map<String, WalletInstance> openWallets = {};
  String? activeWalletId;
  List<OwnedOutput> allOutputs = [];
  List<WalletTransaction> allTransactions = [];
  Set<String> selectedOutputs = {};
  int continuousScanCurrentHeight = 0;

  WalletInstance? get activeWallet =>
      activeWalletId != null ? openWallets[activeWalletId] : null;

  List<WalletInstance> get activeWallets =>
      openWallets.values.where((w) => !w.isClosed && w.walletId != 'temp_wallet').toList();

  /// Check if any active wallets exist without allocating a list.
  bool get hasActiveWallets =>
      openWallets.values.any((w) => !w.isClosed && w.walletId != 'temp_wallet');

  void refreshAvailableWallets() {
    final walletIds = _persistence.listWallets();
    availableWalletIds = walletIds.where((id) => id != 'temp_wallet').toList();
    // Only auto-select when the current non-empty walletId disappeared from
    // the list (e.g. after deletion). Do NOT auto-select when walletId is
    // intentionally empty (e.g. after startNewWallet before an import).
    if (walletIds.isNotEmpty &&
        walletId.isNotEmpty &&
        !walletIds.contains(walletId)) {
      walletId = walletIds.first;
    }
  }

  SwitchResult switchWallet(String newWalletId) {
    if (newWalletId == walletId) {
      return SwitchResult.alreadyCurrent;
    }

    if (openWallets.containsKey(newWalletId) &&
        !openWallets[newWalletId]!.isClosed) {
      switchToWallet(newWalletId);
      return SwitchResult.switchedToOpen;
    }

    walletId = newWalletId;
    refreshAvailableWallets();

    if (_persistence.has(newWalletId)) {
      return SwitchResult.needsLoad;
    } else {
      resetWalletState();
      return SwitchResult.reset;
    }
  }

  WalletInstance? switchToWallet(String id) {
    final wallet = openWallets[id];
    if (wallet == null || wallet.isClosed) return null;

    activeWalletId = id;
    walletId = id;
    allOutputs = wallet.outputs;
    allTransactions = wallet.transactions;
    continuousScanCurrentHeight = wallet.currentHeight;
    return wallet;
  }

  WalletInstance openWallet(
      String id, String seed, String network, String address) {
    final instance = WalletInstance(
      walletId: id,
      seed: seed,
      network: network,
      address: address,
      outputs: [],
      transactions: [],
      currentHeight: 0,
      daemonHeight: 0,
      isScanning: false,
      isClosed: false,
    );

    openWallets[id] = instance;
    activeWalletId = id;
    walletId = id;
    allOutputs = instance.outputs;
    allTransactions = instance.transactions;
    return instance;
  }

  void restoreLoadedData({
    required List<OwnedOutput> outputs,
    required List<WalletTransaction> transactions,
    required Set<String> selectedOutputs,
    required int scanHeight,
    int daemonHeight = 0,
  }) {
    allOutputs = outputs;
    allTransactions = transactions;
    this.selectedOutputs = selectedOutputs;
    continuousScanCurrentHeight = scanHeight;

    if (activeWallet != null) {
      activeWallet!.outputs = outputs;
      activeWallet!.transactions = transactions;
      activeWallet!.currentHeight = scanHeight;
      activeWallet!.daemonHeight = daemonHeight;
    }
  }

  void startNewWallet() {
    walletId = '';
    openWallets.clear();
    activeWalletId = null;
    resetWalletState();
  }

  void resetWalletState() {
    allOutputs = [];
    allTransactions = [];
    selectedOutputs = {};
    continuousScanCurrentHeight = 0;
  }

  int get lowestSyncedHeight {
    int lowest = 0;
    for (final w in openWallets.values) {
      if (w.isClosed || w.walletId == 'temp_wallet') continue;
      if (w.currentHeight > 0 && (lowest == 0 || w.currentHeight < lowest)) {
        lowest = w.currentHeight;
      }
    }
    return lowest;
  }

  CloseWalletResult closeWallet(String walletId) {
    final wallet = openWallets[walletId];
    if (wallet == null) return CloseWalletResult.notFound();

    wallet.isClosed = true;
    wallet.isScanning = false;

    if (activeWalletId == walletId) {
      final remaining = activeWallets;
      if (remaining.isNotEmpty) {
        final next = switchToWallet(remaining.first.walletId);
        return CloseWalletResult.closed(switchedTo: next);
      } else {
        activeWalletId = null;
        allOutputs = [];
        return CloseWalletResult.closed();
      }
    }

    return CloseWalletResult.closed();
  }

  void distributeMultiWalletScanResults({
    required List<WalletScanResult> walletResults,
    required int blockHeight,
    required int daemonHeight,
    required List<String> spentKeyImages,
    List<String> spentKeyImageTxHashes = const [],
    int blockTimestamp = 0,
  }) {
    final updatedWalletAddresses = <String>{};
    // Cache keyImageMap per wallet to avoid rebuilding O(n) map multiple times
    final keyImageMaps = <String, Map<String, OwnedOutput>>{};

    // Build address -> wallet map ONCE for O(1) lookups inside the loop
    final walletsByAddress = <String, WalletInstance>{};
    for (var w in openWallets.values) {
      walletsByAddress[w.address] = w;
    }

    for (var walletResult in walletResults) {
      final walletInstance = walletsByAddress[walletResult.address];
      if (walletInstance == null) continue;

      OutputUtils.addIfAbsent(
          walletInstance.outputs, walletResult.outputs.toList());

      var updatedWallet = _ensureAccountsExist(walletInstance, walletResult.outputs);
      bool accountsUpdated = !identical(updatedWallet, walletInstance);

      WalletInstance activeWalletInstance;
      if (accountsUpdated) {
        openWallets[updatedWallet.walletId] = updatedWallet;
        updatedWallet.currentHeight = blockHeight > updatedWallet.currentHeight ? blockHeight : updatedWallet.currentHeight;
        updatedWallet.daemonHeight = daemonHeight;
        activeWalletInstance = updatedWallet;
      } else {
        if (blockHeight > walletInstance.currentHeight) {
          walletInstance.currentHeight = blockHeight;
        }
        walletInstance.daemonHeight = daemonHeight;
        openWallets[walletInstance.walletId] = walletInstance;
        activeWalletInstance = walletInstance;
      }

      updatedWalletAddresses.add(walletResult.address);

      final walletScanResponse = BlockScanResponse(
        success: true,
        error: null,
        blockHeight: blockHeight,
        blockHash: '',
        blockTimestamp: blockTimestamp,
        txCount: 0,
        outputs: walletResult.outputs,
        daemonHeight: daemonHeight,
        spentKeyImages: spentKeyImages,
        spentKeyImageTxHashes: spentKeyImageTxHashes,
      );

      final walletKeyImageMap = TransactionUtils.buildKeyImageMap(activeWalletInstance.outputs);
      keyImageMaps[activeWalletInstance.walletId] = walletKeyImageMap;
      activeWalletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
        activeWalletInstance.transactions,
        walletScanResponse,
        walletKeyImageMap,
      );
    }

    if (spentKeyImages.isNotEmpty) {
      for (var walletInstance in openWallets.values) {
        if (!updatedWalletAddresses.contains(walletInstance.address)) {
          final walletScanResponse = BlockScanResponse(
            success: true,
            error: null,
            blockHeight: blockHeight,
            blockHash: '',
            blockTimestamp: blockTimestamp,
            txCount: 0,
            outputs: [],
            daemonHeight: daemonHeight,
            spentKeyImages: spentKeyImages,
            spentKeyImageTxHashes: spentKeyImageTxHashes,
          );

          final cachedMap = keyImageMaps[walletInstance.walletId]
              ?? TransactionUtils.buildKeyImageMap(walletInstance.outputs);
          walletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
            walletInstance.transactions,
            walletScanResponse,
            cachedMap,
          );
        }
      }
    }

    for (var walletInstance in openWallets.values) {
      OutputUtils.markSpentByKeyImages(
          walletInstance.outputs, spentKeyImages, selectedOutputs);
    }

    if (activeWalletId != null && activeWallet != null) {
      allOutputs = List<OwnedOutput>.from(activeWallet!.outputs);
      allTransactions = List<WalletTransaction>.from(activeWallet!.transactions);
    }
  }

  void integrateSingleBlockScanResults(BlockScanResponse scanResult) {
    if (activeWalletId == null || activeWallet == null) return;

    final walletInstance = activeWallet!;

    OutputUtils.mergeScannedOutputs(walletInstance.outputs, scanResult.outputs);

    walletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
      walletInstance.transactions,
      scanResult,
      TransactionUtils.buildKeyImageMap(walletInstance.outputs),
    );

    if (scanResult.blockHeight > walletInstance.currentHeight) {
      walletInstance.currentHeight = scanResult.blockHeight;
    }
    walletInstance.daemonHeight = scanResult.daemonHeight;

    var updatedWallet = _ensureAccountsExist(walletInstance, scanResult.outputs);
    bool accountsUpdated = !identical(updatedWallet, walletInstance);

    if (accountsUpdated) {
      openWallets[updatedWallet.walletId] = updatedWallet;
    } else {
      openWallets[walletInstance.walletId] = walletInstance;
    }

    if (scanResult.spentKeyImages.isNotEmpty) {
      final walletToMark = accountsUpdated ? updatedWallet : walletInstance;
      OutputUtils.markSpentByKeyImages(
          walletToMark.outputs, scanResult.spentKeyImages, selectedOutputs);
    }

    allOutputs = List<OwnedOutput>.from(activeWallet!.outputs);
    allTransactions = List<WalletTransaction>.from(activeWallet!.transactions);
  }

  void integrateMempoolScanResults(MempoolScanResponse scanResult, Set<String> selectedOutputKeys) {
    if (activeWalletId == null || activeWallet == null) return;

    final walletInstance = activeWallet!;

    OutputUtils.addIfAbsent(walletInstance.outputs, scanResult.outputs);
    OutputUtils.markSpentByKeyImages(walletInstance.outputs, scanResult.spentKeyImages, selectedOutputKeys);

    var updatedWallet = _ensureAccountsExist(walletInstance, scanResult.outputs);
    bool accountsUpdated = !identical(updatedWallet, walletInstance);

    if (accountsUpdated) {
      openWallets[updatedWallet.walletId] = updatedWallet;
    } else {
      openWallets[walletInstance.walletId] = walletInstance;
    }

    allOutputs = List<OwnedOutput>.from(activeWallet!.outputs);
    allTransactions = List<WalletTransaction>.from(activeWallet!.transactions);
  }

  void handleReorgDetected({
    required int splitHeight,
    required List<String> removedKeyImages,
    required List<String> unspentKeyImages,
  }) {
    if (activeWalletId == null || activeWallet == null) return;

    final wallet = activeWallet!;

    // Remove outputs whose key images were removed (they were in orphaned blocks)
    if (removedKeyImages.isNotEmpty) {
      final removedSet = removedKeyImages.toSet();
      wallet.outputs.removeWhere((o) => removedSet.contains(o.keyImage));
    }

    // Un-spend outputs whose key images were unspent (their spend was in orphaned blocks)
    if (unspentKeyImages.isNotEmpty) {
      final unspentSet = unspentKeyImages.toSet();
      for (int i = 0; i < wallet.outputs.length; i++) {
        if (unspentSet.contains(wallet.outputs[i].keyImage)) {
          wallet.outputs[i] = wallet.outputs[i].copyWith(spent: false);
        }
      }
    }

    // Update wallet height
    if (splitHeight > 0) {
      wallet.currentHeight = splitHeight - 1;
    }

    // Rebuild transactions from remaining outputs
    final outputsByTx = <String, List<OwnedOutput>>{};
    for (var o in wallet.outputs) {
      outputsByTx.putIfAbsent(o.txHash, () => []).add(o);
    }
    wallet.transactions = outputsByTx.entries.map((e) => WalletTransaction(
      txHash: e.key,
      blockHeight: e.value.first.blockHeight,
      blockTimestamp: 0,
      receivedOutputs: e.value,
      spentKeyImages: [],
    )).toList();

    // Refresh public state
    allOutputs = List.from(wallet.outputs);
    allTransactions = List.from(wallet.transactions);
  }

  /// Ensures wallet accounts exist for all subaddress indices in the given outputs.
  /// Returns the same instance if no changes, or a new instance with accounts added.
  WalletInstance _ensureAccountsExist(WalletInstance wallet, Iterable<OwnedOutput> outputs) {
    int highestAccountIndex = 0;
    for (var output in outputs) {
      if (output.subaddressIndex != null) {
        final accountIndex = output.subaddressIndex!.$1;
        if (accountIndex > highestAccountIndex) {
          highestAccountIndex = accountIndex;
        }
      }
    }

    var updated = wallet;
    final existingAccounts = Set<int>.from(updated.accounts);
    for (int i = 0; i <= highestAccountIndex; i++) {
      if (!existingAccounts.contains(i)) {
        updated = updated.createAccount(i);
        existingAccounts.add(i);
      }
    }
    return updated;
  }
}
