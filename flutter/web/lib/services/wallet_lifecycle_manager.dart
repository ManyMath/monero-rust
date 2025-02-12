import '../src/bindings/bindings.dart';
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
      openWallets.values.where((w) => !w.isClosed).toList();

  void refreshAvailableWallets() {
    final walletIds = _persistence.listWallets();
    availableWalletIds = walletIds;
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
    final heights = activeWallets
        .where((w) => w.currentHeight > 0)
        .map((w) => w.currentHeight)
        .toList();
    return heights.isEmpty ? 0 : heights.reduce((a, b) => a < b ? a : b);
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
    int blockTimestamp = 0,
  }) {
    // Track which wallets have been updated with new outputs
    final updatedWalletAddresses = <String>{};

    // Update outputs and transactions per wallet
    for (var walletResult in walletResults) {
      final walletInstance = openWallets.values.cast<WalletInstance?>().firstWhere(
        (w) => w != null && w.address == walletResult.address,
        orElse: () => null,
      );

      if (walletInstance != null) {
        OutputUtils.addIfAbsent(
            walletInstance.outputs, walletResult.outputs.toList());

        if (blockHeight > walletInstance.currentHeight) {
          walletInstance.currentHeight = blockHeight;
        }
        walletInstance.daemonHeight = daemonHeight;

        updatedWalletAddresses.add(walletResult.address);

        // Update transactions for this specific wallet
        final walletScanResponse = BlockScanResponse(
          success: true,
          error: null,
          blockHeight: Uint64(BigInt.from(blockHeight)),
          blockHash: '',
          blockTimestamp: Uint64(BigInt.from(blockTimestamp)),
          txCount: 0,
          outputs: walletResult.outputs,
          daemonHeight: Uint64(BigInt.from(daemonHeight)),
          spentKeyImages: spentKeyImages,
        );

        walletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
          walletInstance.transactions,
          walletScanResponse,
          walletInstance.outputs,
        );
      }
    }

    // Update all wallets with spent key images (even those without new outputs)
    if (spentKeyImages.isNotEmpty) {
      for (var walletInstance in openWallets.values) {
        // Skip wallets that were already updated above
        if (!updatedWalletAddresses.contains(walletInstance.address)) {
          // Update transactions for spent key images
          final walletScanResponse = BlockScanResponse(
            success: true,
            error: null,
            blockHeight: Uint64(BigInt.from(blockHeight)),
            blockHash: '',
            blockTimestamp: Uint64(BigInt.from(blockTimestamp)),
            txCount: 0,
            outputs: [],
            daemonHeight: Uint64(BigInt.from(daemonHeight)),
            spentKeyImages: spentKeyImages,
          );

          walletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
            walletInstance.transactions,
            walletScanResponse,
            walletInstance.outputs,
          );
        }
      }
    }

    for (var walletInstance in openWallets.values) {
      OutputUtils.markSpentByKeyImages(
          walletInstance.outputs, spentKeyImages, selectedOutputs);
    }

    if (activeWalletId != null && activeWallet != null) {
      allOutputs = activeWallet!.outputs;
      allTransactions = activeWallet!.transactions;
    }
  }
}
