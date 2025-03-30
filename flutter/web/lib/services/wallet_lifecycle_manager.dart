import 'package:tuple/tuple.dart';
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
    print('\n========== START distributeMultiWalletScanResults ==========');
    print('Block height: $blockHeight');
    print('Wallet results count: ${walletResults.length}');
    
    // Track which wallets have been updated with new outputs
    final updatedWalletAddresses = <String>{};

    // Update outputs and transactions per wallet
    for (var walletResult in walletResults) {
      print('\n--- Processing wallet: ${walletResult.address.substring(0, 20)}...');
      print('Outputs for this wallet: ${walletResult.outputs.length}');
      
      final walletInstance = openWallets.values.cast<WalletInstance?>().firstWhere(
        (w) => w != null && w.address == walletResult.address,
        orElse: () => null,
      );

      if (walletInstance != null) {
        print('walletInstance.outputs.length BEFORE addIfAbsent: ${walletInstance.outputs.length}');
        OutputUtils.addIfAbsent(
            walletInstance.outputs, walletResult.outputs.toList());
        print('walletInstance.outputs.length AFTER addIfAbsent: ${walletInstance.outputs.length}');

        int highestAccountIndex = 0;
        for (var output in walletResult.outputs) {
          if (output.subaddressIndex != null) {
            final accountIndex = output.subaddressIndex!.item1;
            if (accountIndex > highestAccountIndex) {
              highestAccountIndex = accountIndex;
            }
          }
        }
        print('Highest account index: $highestAccountIndex');

        var updatedWallet = walletInstance;
        bool accountsUpdated = false;
        for (int i = 0; i <= highestAccountIndex; i++) {
          if (!updatedWallet.accounts.contains(i)) {
            print('Creating new account: $i');
            print('updatedWallet.outputs.length BEFORE createAccount: ${updatedWallet.outputs.length}');
            updatedWallet = updatedWallet.createAccount(i);
            print('updatedWallet.outputs.length AFTER createAccount: ${updatedWallet.outputs.length}');
            print('identical(walletInstance.outputs, updatedWallet.outputs): ${identical(walletInstance.outputs, updatedWallet.outputs)}');
            accountsUpdated = true;
          }
        }

        WalletInstance activeWalletInstance = walletInstance;
        if (accountsUpdated) {
          print('Accounts updated, replacing in openWallets');
          print('openWallets[${updatedWallet.walletId}].outputs.length BEFORE: ${openWallets[updatedWallet.walletId]?.outputs.length}');
          openWallets[updatedWallet.walletId] = updatedWallet;
          print('openWallets[${updatedWallet.walletId}].outputs.length AFTER: ${openWallets[updatedWallet.walletId]?.outputs.length}');
          updatedWallet.currentHeight = blockHeight > updatedWallet.currentHeight ? blockHeight : updatedWallet.currentHeight;
          updatedWallet.daemonHeight = daemonHeight;
          activeWalletInstance = updatedWallet;
        } else {
          if (blockHeight > walletInstance.currentHeight) {
            walletInstance.currentHeight = blockHeight;
          }
          walletInstance.daemonHeight = daemonHeight;
          openWallets[walletInstance.walletId] = walletInstance;
        }

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

        activeWalletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
          activeWalletInstance.transactions,
          walletScanResponse,
          activeWalletInstance.outputs,
        );
        print('activeWalletInstance.transactions.length: ${activeWalletInstance.transactions.length}');
      } else {
        print('WARNING: Wallet instance not found for address');
      }
    }

    // Update all wallets with spent key images (even those without new outputs)
    if (spentKeyImages.isNotEmpty) {
      print('\n--- Processing spent key images ---');
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

    print('\n--- Marking spent outputs ---');
    for (var walletInstance in openWallets.values) {
      OutputUtils.markSpentByKeyImages(
          walletInstance.outputs, spentKeyImages, selectedOutputs);
    }

    if (activeWalletId != null && activeWallet != null) {
      print('\n--- Updating allOutputs and allTransactions ---');
      print('activeWallet.outputs.length: ${activeWallet?.outputs.length}');
      // Create new list instances to trigger Flutter's change detection
      allOutputs = List<OwnedOutput>.from(activeWallet!.outputs);
      allTransactions = List<WalletTransaction>.from(activeWallet!.transactions);
      print('allOutputs.length: ${allOutputs.length}');
      print('allTransactions.length: ${allTransactions.length}');
    }
    
    print('========== END distributeMultiWalletScanResults ==========\n');
  }

  /// Integrates single block scan results into the active wallet instance
  void integrateSingleBlockScanResults(BlockScanResponse scanResult) {
    print('\n========== START integrateSingleBlockScanResults ==========');
    print('Block height: ${scanResult.blockHeight.toInt()}');
    print('Incoming outputs count: ${scanResult.outputs.length}');
    
    if (activeWalletId == null || activeWallet == null) {
      print('ERROR: No active wallet to integrate scan results');
      print('========== END integrateSingleBlockScanResults ==========\n');
      return;
    }

    print('Active wallet ID: $activeWalletId');
    final walletInstance = activeWallet!;
    print('walletInstance.hashCode: ${walletInstance.hashCode}');
    print('walletInstance.walletId: ${walletInstance.walletId}');
    print('walletInstance.outputs.hashCode BEFORE merge: ${walletInstance.outputs.hashCode}');
    print('walletInstance.outputs.length BEFORE merge: ${walletInstance.outputs.length}');
    print('openWallets[activeWalletId].hashCode: ${openWallets[activeWalletId].hashCode}');
    print('openWallets[activeWalletId].outputs.hashCode: ${openWallets[activeWalletId]!.outputs.hashCode}');
    print('walletInstance === openWallets[activeWalletId]: ${identical(walletInstance, openWallets[activeWalletId])}');
    print('walletInstance.outputs === openWallets[activeWalletId].outputs: ${identical(walletInstance.outputs, openWallets[activeWalletId]!.outputs)}');

    // Add new outputs to the wallet instance
    print('\n--- Calling mergeScannedOutputs ---');
    OutputUtils.mergeScannedOutputs(walletInstance.outputs, scanResult.outputs);
    print('walletInstance.outputs.length AFTER merge: ${walletInstance.outputs.length}');
    print('walletInstance.outputs.hashCode AFTER merge: ${walletInstance.outputs.hashCode}');
    print('openWallets[activeWalletId].outputs.length AFTER merge: ${openWallets[activeWalletId]!.outputs.length}');
    print('openWallets[activeWalletId].outputs.hashCode AFTER merge: ${openWallets[activeWalletId]!.outputs.hashCode}');

    // Update transactions in the wallet instance
    print('\n--- Updating transactions ---');
    walletInstance.transactions = TransactionUtils.updateTransactionsFromScan(
      walletInstance.transactions,
      scanResult,
      walletInstance.outputs,
    );
    print('walletInstance.transactions.length: ${walletInstance.transactions.length}');

    // Update wallet heights
    final blockHeight = scanResult.blockHeight.toInt();
    if (blockHeight > walletInstance.currentHeight) {
      walletInstance.currentHeight = blockHeight;
    }
    walletInstance.daemonHeight = scanResult.daemonHeight.toInt();
    print('walletInstance.currentHeight: ${walletInstance.currentHeight}');
    print('walletInstance.daemonHeight: ${walletInstance.daemonHeight}');

    // Ensure accounts exist for new outputs
    print('\n--- Checking for new accounts ---');
    int highestAccountIndex = 0;
    for (var output in scanResult.outputs) {
      if (output.subaddressIndex != null) {
        final accountIndex = output.subaddressIndex!.item1;
        if (accountIndex > highestAccountIndex) {
          highestAccountIndex = accountIndex;
        }
      }
    }
    print('Highest account index in outputs: $highestAccountIndex');
    print('Current accounts: ${walletInstance.accounts}');

    var updatedWallet = walletInstance;
    bool accountsUpdated = false;
    for (int i = 0; i <= highestAccountIndex; i++) {
      if (!updatedWallet.accounts.contains(i)) {
        print('Creating new account: $i');
        print('updatedWallet.hashCode BEFORE createAccount($i): ${updatedWallet.hashCode}');
        print('updatedWallet.outputs.hashCode BEFORE createAccount($i): ${updatedWallet.outputs.hashCode}');
        print('updatedWallet.outputs.length BEFORE createAccount($i): ${updatedWallet.outputs.length}');
        
        final oldWallet = updatedWallet;
        updatedWallet = updatedWallet.createAccount(i);
        
        print('updatedWallet.hashCode AFTER createAccount($i): ${updatedWallet.hashCode}');
        print('updatedWallet.outputs.hashCode AFTER createAccount($i): ${updatedWallet.outputs.hashCode}');
        print('updatedWallet.outputs.length AFTER createAccount($i): ${updatedWallet.outputs.length}');
        print('oldWallet === updatedWallet: ${identical(oldWallet, updatedWallet)}');
        print('oldWallet.outputs === updatedWallet.outputs: ${identical(oldWallet.outputs, updatedWallet.outputs)}');
        
        accountsUpdated = true;
      }
    }

    if (accountsUpdated) {
      print('\n--- Accounts were updated, replacing in openWallets ---');
      print('openWallets[${updatedWallet.walletId}].hashCode BEFORE replacement: ${openWallets[updatedWallet.walletId]?.hashCode}');
      print('openWallets[${updatedWallet.walletId}].outputs.length BEFORE replacement: ${openWallets[updatedWallet.walletId]?.outputs.length}');
      
      openWallets[updatedWallet.walletId] = updatedWallet;
      
      print('openWallets[${updatedWallet.walletId}].hashCode AFTER replacement: ${openWallets[updatedWallet.walletId]?.hashCode}');
      print('openWallets[${updatedWallet.walletId}].outputs.length AFTER replacement: ${openWallets[updatedWallet.walletId]?.outputs.length}');
      print('openWallets[${updatedWallet.walletId}].outputs.hashCode AFTER replacement: ${openWallets[updatedWallet.walletId]?.outputs.hashCode}');
    } else {
      print('\n--- No new accounts created ---');
      openWallets[walletInstance.walletId] = walletInstance;
    }

    // Update the derived lists to ensure UI gets fresh references
    print('\n--- Updating allOutputs and allTransactions ---');
    print('activeWallet before final copy:');
    print('  activeWallet.hashCode: ${activeWallet?.hashCode}');
    print('  activeWallet.outputs.hashCode: ${activeWallet?.outputs.hashCode}');
    print('  activeWallet.outputs.length: ${activeWallet?.outputs.length}');
    print('  activeWallet === walletInstance: ${identical(activeWallet, walletInstance)}');
    print('  activeWallet === updatedWallet: ${identical(activeWallet, updatedWallet)}');
    print('  activeWallet === openWallets[activeWalletId]: ${identical(activeWallet, openWallets[activeWalletId])}');
    
    // Force new list references to trigger UI updates
    allOutputs = List<OwnedOutput>.from(activeWallet!.outputs);
    allTransactions = List<WalletTransaction>.from(activeWallet!.transactions);
    
    print('allOutputs.length: ${allOutputs.length}');
    print('allTransactions.length: ${allTransactions.length}');
    print('allOutputs.hashCode: ${allOutputs.hashCode}');
    
    print('\n--- Final verification ---');
    print('walletInstance.outputs.length: ${walletInstance.outputs.length}');
    print('updatedWallet.outputs.length: ${updatedWallet.outputs.length}');
    print('openWallets[activeWalletId].outputs.length: ${openWallets[activeWalletId]?.outputs.length}');
    print('activeWallet.outputs.length: ${activeWallet?.outputs.length}');
    print('allOutputs.length: ${allOutputs.length}');
    
    print('========== END integrateSingleBlockScanResults ==========\n');
  }

  /// Integrates mempool scan results into the active wallet instance
  void integrateMempoolScanResults(MempoolScanResponse scanResult, Set<String> selectedOutputKeys) {
    print('\n========== START integrateMempoolScanResults ==========');
    print('Incoming outputs count: ${scanResult.outputs.length}');
    
    if (activeWalletId == null || activeWallet == null) {
      print('ERROR: No active wallet to integrate mempool scan results');
      print('========== END integrateMempoolScanResults ==========\n');
      return;
    }

    print('Active wallet ID: $activeWalletId');
    final walletInstance = activeWallet!;
    print('walletInstance.outputs.length BEFORE merge: ${walletInstance.outputs.length}');

    // Add new outputs to the wallet instance
    OutputUtils.addIfAbsent(walletInstance.outputs, scanResult.outputs);
    print('walletInstance.outputs.length AFTER merge: ${walletInstance.outputs.length}');

    // Mark spent outputs based on key images from mempool
    OutputUtils.markSpentByKeyImages(walletInstance.outputs, scanResult.spentKeyImages, selectedOutputKeys);

    // Ensure accounts exist for new outputs
    int highestAccountIndex = 0;
    for (var output in scanResult.outputs) {
      if (output.subaddressIndex != null) {
        final accountIndex = output.subaddressIndex!.item1;
        if (accountIndex > highestAccountIndex) {
          highestAccountIndex = accountIndex;
        }
      }
    }
    print('Highest account index in outputs: $highestAccountIndex');

    var updatedWallet = walletInstance;
    bool accountsUpdated = false;
    for (int i = 0; i <= highestAccountIndex; i++) {
      if (!updatedWallet.accounts.contains(i)) {
        print('Creating new account: $i');
        print('updatedWallet.outputs.length BEFORE createAccount($i): ${updatedWallet.outputs.length}');
        updatedWallet = updatedWallet.createAccount(i);
        print('updatedWallet.outputs.length AFTER createAccount($i): ${updatedWallet.outputs.length}');
        accountsUpdated = true;
      }
    }

    if (accountsUpdated) {
      print('Accounts updated, replacing in openWallets');
      openWallets[updatedWallet.walletId] = updatedWallet;
    } else {
      openWallets[walletInstance.walletId] = walletInstance;
    }

    // Update the derived lists to ensure UI gets fresh references
    print('Updating allOutputs from activeWallet');
    print('activeWallet.outputs.length: ${activeWallet?.outputs.length}');
    allOutputs = List<OwnedOutput>.from(activeWallet!.outputs);
    allTransactions = List<WalletTransaction>.from(activeWallet!.transactions);
    print('allOutputs.length after copy: ${allOutputs.length}');
    
    print('========== END integrateMempoolScanResults ==========\n');
  }
}
