import 'dart:async';
import 'dart:html' as html;
import 'package:flutter/material.dart';
import 'package:monero_extension/utils/network_utils.dart';
import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';
import '../services/extension_service.dart';
import '../widgets/password_dialog.dart';
import '../widgets/wallet_id_dialog.dart';
import '../widgets/save_wallet_dialog.dart';
import '../widgets/delete_confirmation_dialog.dart';
import '../services/wallet_persistence_browser.dart';
import '../models/wallet_instance.dart';
import '../models/wallet_transaction.dart';
import '../utils/clipboard_utils.dart';
import '../utils/balance_utils.dart';
import '../utils/output_utils.dart';
import '../utils/transaction_utils.dart';
import '../services/wallet_scan_service.dart';
import '../services/transaction_service.dart';
import '../services/wallet_polling_service.dart';
import '../widgets/payment_proof_dialog.dart';
import '../widgets/keys_display_panel.dart';
import '../widgets/receive_panel.dart';
import '../widgets/scanning_panel.dart';
import '../widgets/transactions_panel.dart';
import '../widgets/outputs_panel.dart';
import '../widgets/seed_phrase_panel.dart';
import '../widgets/file_management_panel.dart';
import '../widgets/close_wallet_dialog.dart';
import '../widgets/create_transaction_panel.dart';
import '../widgets/overwrite_wallet_dialog.dart';
import '../widgets/security_warning_dialog.dart';
import '../services/wallet_lifecycle_manager.dart';

enum DebugPanel {
  fileManagement('File Management'),
  seedPhrase('Seed Phrase'),
  keys('Keys'),
  receive('Receive'),
  scanning('Scanning'),
  transactions('Transactions'),
  coins('Coins'),
  createTransaction('Send');

  final String title;
  const DebugPanel(this.title);

  static DebugPanel? fromIndex(int? index) {
    if (index == null) return null;
    if (index < 0 || index >= DebugPanel.values.length) return null;
    return DebugPanel.values[index];
  }
}

class DebugView extends StatefulWidget {
  const DebugView({super.key});

  @override
  State<DebugView> createState() => _DebugViewState();
}

class _DebugViewState extends State<DebugView> {
  final _controller = TextEditingController();
  final _extensionService = ExtensionService();
  final _pollingService = WalletPollingService();
  final _nodeUrlController = TextEditingController(text: 'http://127.0.0.1:38081');
  final _blockHeightController = TextEditingController();
  final _blockHeightFocusNode = FocusNode();
  bool _blockHeightUserEdited = false;

  // Lifecycle manager: single source of truth for wallet state
  late final WalletLifecycleManager _lifecycle;

  // Delegating getters/setters for wallet state
  String get _walletId => _lifecycle.walletId;
  set _walletId(String v) => _lifecycle.walletId = v;
  List<String> get _availableWalletIds => _lifecycle.availableWalletIds;
  Map<String, WalletInstance> get _openWallets => _lifecycle.openWallets;
  String? get _activeWalletId => _lifecycle.activeWalletId;
  List<WalletInstance> get _activeWallets => _lifecycle.activeWallets;

  int get _lowestSyncedHeight => _lifecycle.lowestSyncedHeight;

  String _network = 'stagenet';
  String _seedType = '25 word (classic)';
  String? _validationError;
  String? _derivedAddress;
  String? _responseError;
  bool _isScanning = false;
  Timer? _debounceTimer;
  String? _secretSpendKey;
  String? _secretViewKey;
  String? _publicSpendKey;
  String? _publicViewKey;

  // Account management state
  // Get active account from the wallet
  int get _activeAccount {
    final activeWallet = _lifecycle.activeWallet;
    return activeWallet?.activeAccount ?? 0;
  }
  Map<String, String> _subaddresses = {}; // "account,index" -> address
  final Set<String> _pendingSubaddresses = {}; // Track pending derivations

  // Get accounts from the active wallet
  List<int> get _accounts {
    final activeWallet = _lifecycle.activeWallet;
    return activeWallet?.accounts ?? [0];
  }

  BlockScanResponse? _scanResult;
  String? _scanError;

  // All outputs across all accounts
  List<OwnedOutput> get _allOutputsAllAccounts => _lifecycle.allOutputs;
  set _allOutputsAllAccounts(List<OwnedOutput> v) => _lifecycle.allOutputs = v;

  // Filtered outputs for active account only (or all accounts if _activeAccount == -1)
  List<OwnedOutput> get _allOutputs {
    // If "All" is selected, return all outputs
    if (_activeAccount == -1) {
      return _allOutputsAllAccounts;
    }

    return _allOutputsAllAccounts.where((output) {
      if (output.subaddressIndex == null) {
        // Outputs without subaddress index belong to account 0, address 0
        return _activeAccount == 0;
      }
      return output.subaddressIndex!.item1 == _activeAccount;
    }).toList();
  }

  int? _daemonHeight;

  // Transaction tracking state
  List<WalletTransaction> get _allTransactionsAllAccounts => _lifecycle.allTransactions;
  set _allTransactionsAllAccounts(List<WalletTransaction> v) => _lifecycle.allTransactions = v;

  // Filtered transactions for active account only (or all accounts if _activeAccount == -1)
  List<WalletTransaction> get _allTransactions {
    // If "All" is selected, return all transactions
    if (_activeAccount == -1) {
      return _allTransactionsAllAccounts;
    }

    return _allTransactionsAllAccounts.where((tx) {
      // A transaction is relevant to this account if it has any received outputs for this account
      final hasReceivedOutputs = tx.receivedOutputs.any((output) {
        if (output.subaddressIndex == null) {
          return _activeAccount == 0;
        }
        return output.subaddressIndex!.item1 == _activeAccount;
      });

      // OR if it spent outputs from this account
      final hasSpentOutputs = tx.spentKeyImages.any((keyImage) {
        final spentOutput = _allOutputsAllAccounts.where((o) => o.keyImage == keyImage).firstOrNull;
        if (spentOutput == null) return false;
        if (spentOutput.subaddressIndex == null) {
          return _activeAccount == 0;
        }
        return spentOutput.subaddressIndex!.item1 == _activeAccount;
      });

      return hasReceivedOutputs || hasSpentOutputs;
    }).toList();
  }
  String _txSortBy = 'confirms'; // 'confirms' or 'amount'
  bool _txSortAscending = false;
  Set<String> _expandedTransactions = {}; // Track which transaction cards are expanded

  // Current blockchain height (defaults to 0)
  int get _currentHeight => _daemonHeight ?? _scanResult?.blockHeight.toInt() ?? 0;

  // Continuous scan state
  bool _isContinuousScanning = false;
  bool _isContinuousPaused = false;
  int get _continuousScanCurrentHeight => _lifecycle.continuousScanCurrentHeight;
  set _continuousScanCurrentHeight(int v) => _lifecycle.continuousScanCurrentHeight = v;
  int _continuousScanTargetHeight = 0;
  bool _isSynced = false;

  final List<TextEditingController> _destinationControllers = [TextEditingController()];
  final List<TextEditingController> _amountControllers = [TextEditingController()];
  bool _isCreatingTx = false;
  TransactionCreatedResponse? _txResult;
  String? _txError;

  bool _isBroadcasting = false;
  TransactionBroadcastResponse? _broadcastResult;
  String? _broadcastError;

  bool _showSpentOutputs = false;
  String _sortBy = 'confirms'; // 'confirms' or 'value'
  bool _sortAscending = false; // false = descending (highest first)
  Set<String> get _selectedOutputs => _lifecycle.selectedOutputs;
  set _selectedOutputs(Set<String> v) => _lifecycle.selectedOutputs = v;

  bool _isScanningMempool = false;

  // File management state
  bool _isSaving = false;
  bool _isLoadingWallet = false;
  String? _saveError;
  String? _loadError;
  String? _lastSaveTime;
  bool _isExporting = false;
  bool _isImporting = false;
  bool _isRestoringWallet = false;
  String? _exportError;
  String? _importError;

  DebugPanel? _expandedPanel;

  // Stream subscriptions
  StreamSubscription? _keysDerivedSubscription;
  StreamSubscription? _subaddressDerivedSubscription;
  StreamSubscription? _seedGeneratedSubscription;
  StreamSubscription? _blockScanSubscription;
  StreamSubscription? _daemonHeightSubscription;
  StreamSubscription? _transactionCreatedSubscription;
  StreamSubscription? _transactionBroadcastSubscription;
  StreamSubscription? _syncProgressSubscription;
  StreamSubscription? _spentStatusUpdatedSubscription;
  StreamSubscription? _mempoolScanSubscription;
  StreamSubscription? _multiWalletScanSubscription;

  @override
  void initState() {
    super.initState();

    _lifecycle = WalletLifecycleManager(
      persistence: WalletPersistenceBrowser.defaultPersistence,
    );

    _controller.addListener(_onSeedChanged);
    _blockHeightController.addListener(_onBlockHeightChanged);

    _keysDerivedSubscription = KeysDerivedResponse.rustSignalStream.listen((signal) {
      setState(() {
        if (signal.message.success) {
          _derivedAddress = signal.message.address;
          _secretSpendKey = signal.message.secretSpendKey;
          _secretViewKey = signal.message.secretViewKey;
          _publicSpendKey = signal.message.publicSpendKey;
          _publicViewKey = signal.message.publicViewKey;
          _responseError = null;

          // Open wallet when keys are derived from seed
          final seed = _controller.text.trim();
          if (seed.isNotEmpty && _derivedAddress != null && _lifecycle.activeWallet == null) {
            _openWallet(_walletId.isEmpty ? 'temp_wallet' : _walletId, seed, _network, _derivedAddress!);
          }
        } else {
          _derivedAddress = null;
          _secretSpendKey = null;
          _secretViewKey = null;
          _publicSpendKey = null;
          _publicViewKey = null;
          _responseError = signal.message.error ?? 'Unknown error';
        }
      });
    });

    _subaddressDerivedSubscription = SubaddressDerivedResponse.rustSignalStream.listen((signal) {
      if (signal.message.success && signal.message.address.isNotEmpty) {
        setState(() {
          // Match the response to the first pending request
          if (_pendingSubaddresses.isNotEmpty) {
            final key = _pendingSubaddresses.first;
            _subaddresses[key] = signal.message.address;
            _pendingSubaddresses.remove(key);
          }
        });
      }
    });

    _seedGeneratedSubscription = SeedGeneratedResponse.rustSignalStream.listen((signal) {
      if (signal.message.success) {
        setState(() {
          _controller.text = signal.message.seed;
          _validationError = null;
          _responseError = null;
          _derivedAddress = null;

          // Auto-populate block height for polyseed if available
          if (signal.message.restoreHeight != null) {
            final timestamp = signal.message.restoreHeight!.toInt();
            if (timestamp > 0) {
              final genesisTimestamp = _getGenesisTimestamp(_network);
              final approxHeight = ((timestamp - genesisTimestamp) / 120).toInt();
              // Subtract safety margin (~720 blocks = ~1 day) to avoid missing transactions
              final safeHeight = (approxHeight - 720).clamp(0, approxHeight);
              _blockHeightController.text = safeHeight.toString();
              _blockHeightUserEdited = false;
            }
          }
        });
      } else {
        setState(() {
          _responseError = signal.message.error ?? 'Failed to generate seed';
        });
      }
    });

    _blockScanSubscription = BlockScanResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isScanning = false;
        if (signal.message.success) {
          _scanResult = signal.message;
          _scanError = null;
          _daemonHeight = signal.message.daemonHeight.toInt();

          // Integrate scan results into the active wallet instance
          _lifecycle.integrateSingleBlockScanResults(signal.message);
        } else {
          _scanResult = null;
          _scanError = signal.message.error ?? 'Unknown error during scan';
        }
      });
    });

    _daemonHeightSubscription = DaemonHeightResponse.rustSignalStream.listen((signal) {
      if (signal.message.success) {
        setState(() {
          _daemonHeight = signal.message.daemonHeight.toInt();
        });
      } else {
        setState(() {
          _scanError = signal.message.error ?? 'Failed to get daemon height';
        });
      }
    });

    _transactionCreatedSubscription = TransactionCreatedResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isCreatingTx = false;
        if (signal.message.success) {
          _txResult = signal.message;
          _txError = null;
          _broadcastResult = null;
          _broadcastError = null;

          // Add change outputs as unconfirmed (blockHeight=0)
          final changeOwned = signal.message.changeOutputs.map(OutputUtils.changeOutputToOwned).toList();
          OutputUtils.addIfAbsent(_allOutputsAllAccounts, changeOwned);
        } else {
          _txResult = null;
          _txError = signal.message.error ?? 'Unknown error during transaction creation';
        }
      });
    });

    _transactionBroadcastSubscription = TransactionBroadcastResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isBroadcasting = false;
        if (signal.message.success) {
          _broadcastResult = signal.message;
          _broadcastError = null;
          // Mark spent outputs immediately after broadcast
          if (_txResult != null) {
            OutputUtils.markSpentByOutputKeys(_allOutputsAllAccounts, _txResult!.spentOutputHashes, _selectedOutputs);
          }
        } else {
          _broadcastResult = null;
          _broadcastError = signal.message.error ?? 'Unknown error during broadcast';
        }
      });
    });

    _syncProgressSubscription = SyncProgressResponse.rustSignalStream.listen((signal) {
      final wasSynced = _isSynced;
      final wasScanning = _isContinuousScanning;
      setState(() {
        _continuousScanCurrentHeight = signal.message.currentHeight.toInt();
        _continuousScanTargetHeight = signal.message.daemonHeight.toInt();
        _isSynced = signal.message.isSynced;
        if (!_isContinuousPaused) {
          _isContinuousScanning = signal.message.isScanning;
        } else if (!signal.message.isScanning) {
          _isContinuousScanning = false;
        }
        if (_isContinuousScanning && !wasScanning) {
          _isContinuousPaused = false;
        }
        if (!_blockHeightFocusNode.hasFocus) {
          _blockHeightController.text = _continuousScanCurrentHeight.toString();
          _blockHeightUserEdited = false;
        }
      });

      // Stop polling timers when continuous scanning starts
      if (_isContinuousScanning && !wasScanning) {
        _stopPollingTimers();
      }
      // Start polling timers when:
      // - Sync completes
      // - Continuous scan finishes or is paused
      // - We're synced and not actively scanning
      if (!_isContinuousScanning && (wasScanning || (_isSynced && !wasSynced))) {
        _startPollingTimers();
      }
    });

    _spentStatusUpdatedSubscription = SpentStatusUpdatedResponse.rustSignalStream.listen((signal) {
      setState(() {
        OutputUtils.markSpentByKeyImages(_allOutputsAllAccounts, signal.message.spentKeyImages, _selectedOutputs);
      });
    });

    _mempoolScanSubscription = MempoolScanResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isScanningMempool = false;
        if (signal.message.success) {
          OutputUtils.addIfAbsent(_allOutputsAllAccounts, signal.message.outputs);
          OutputUtils.markSpentByKeyImages(_allOutputsAllAccounts, signal.message.spentKeyImages, _selectedOutputs);

          _ensureAccountsExistForOutputs(signal.message.outputs);
        }
      });
    });

    _multiWalletScanSubscription = MultiWalletScanResponse.rustSignalStream.listen((signalPack) {
      final response = signalPack.message;

      if (!response.success) {
        setState(() {
          _scanError = response.error;
        });
        return;
      }

      setState(() {
        _daemonHeight = response.daemonHeight.toInt();
        _lifecycle.distributeMultiWalletScanResults(
          walletResults: response.walletResults,
          blockHeight: response.blockHeight.toInt(),
          daemonHeight: response.daemonHeight.toInt(),
          spentKeyImages: response.spentKeyImages,
          blockTimestamp: response.blockTimestamp.toInt(),
        );
      });
      _updateBlockHeightFromWallets();
    });

    // Load available wallets from localStorage
    _refreshAvailableWallets();
  }

  void _startPollingTimers() {
    _pollingService.startPolling(
      onBlockRefresh: _onBlockRefreshTimer,
      onMempoolPoll: _onMempoolPollTimer,
      onCountdownUpdate: () {
        setState(() {}); // Trigger UI update for countdown changes
      },
    );
  }

  void _stopPollingTimers() {
    _pollingService.stopPolling();
  }

  /// Normalizes a node URL by trimming whitespace and adding http:// if no scheme is present.
  String _normalizeNodeUrl(String url) {
    return NetworkUtils.normalizeNodeUrl(url);
  }

  void _onBlockRefreshTimer() {
    if (_isContinuousPaused || !_isContinuousScanning) {
      return;
    }

    final nodeUrl = _normalizeNodeUrl(_nodeUrlController.text);
    final walletsToScan = _activeWallets;

    if (walletsToScan.isEmpty) return;

    WalletScanService.queryDaemonHeight(nodeUrl);

    if (walletsToScan.length > 1) {
      final walletConfigs = walletsToScan.map((w) => w.toWalletConfig()).toList();
      StartMultiWalletScanRequest(
        nodeUrl: nodeUrl,
        startHeight: Uint64(BigInt.from(_continuousScanCurrentHeight)),
        wallets: walletConfigs,
      ).sendSignalToRust();
    } else {
      final wallet = walletsToScan.first;
      // Get the highest account index for this wallet
      final highestAccount = _accounts.isEmpty ? 0 : _accounts.reduce((a, b) => a > b ? a : b);
      StartContinuousScanRequest(
        nodeUrl: nodeUrl,
        startHeight: Uint64(BigInt.from(_continuousScanCurrentHeight)),
        seed: wallet.seed,
        network: wallet.network,
        accountLookahead: highestAccount,
      ).sendSignalToRust();
    }
  }

  void _onMempoolPollTimer() {
    final seed = _controller.text.trim();
    if (seed.isEmpty) return;

    final nodeUrl = _normalizeNodeUrl(_nodeUrlController.text);
    // Get the highest account index for mempool scanning
    final highestAccount = _accounts.isEmpty ? 0 : _accounts.reduce((a, b) => a > b ? a : b);

    MempoolScanRequest(
      nodeUrl: nodeUrl,
      seed: seed,
      network: _network,
      accountLookahead: highestAccount,
    ).sendSignalToRust();
  }

  @override
  void dispose() {
    // Cancel stream subscriptions
    _keysDerivedSubscription?.cancel();
    _subaddressDerivedSubscription?.cancel();
    _seedGeneratedSubscription?.cancel();
    _blockScanSubscription?.cancel();
    _daemonHeightSubscription?.cancel();
    _transactionCreatedSubscription?.cancel();
    _transactionBroadcastSubscription?.cancel();
    _syncProgressSubscription?.cancel();
    _spentStatusUpdatedSubscription?.cancel();
    _mempoolScanSubscription?.cancel();
    _multiWalletScanSubscription?.cancel();

    _stopPollingTimers();
    _debounceTimer?.cancel();
    _controller.removeListener(_onSeedChanged);
    _blockHeightController.removeListener(_onBlockHeightChanged);
    _controller.dispose();
    _nodeUrlController.dispose();
    _blockHeightController.dispose();
    _blockHeightFocusNode.dispose();
    for (var c in _destinationControllers) {
      c.dispose();
    }
    for (var c in _amountControllers) {
      c.dispose();
    }
    super.dispose();
  }

  void _onSeedChanged() {
    if (_isRestoringWallet) return;
    _debounceTimer?.cancel();

    if (_isContinuousScanning) {
      StopScanRequest().sendSignalToRust();
    }
    setState(() {
      _continuousScanCurrentHeight = 0;
      _continuousScanTargetHeight = 0;
      _isSynced = false;
      _allOutputsAllAccounts = [];
      _allTransactionsAllAccounts = [];
      _expandedTransactions = {};
      _selectedOutputs = {};
      _daemonHeight = null;
      _scanResult = null;
    });

    _debounceTimer = Timer(const Duration(milliseconds: 800), () {
      _deriveAddress();
    });
  }

  void _onBlockHeightChanged() {
    if (_blockHeightFocusNode.hasFocus) {
      _blockHeightUserEdited = true;
    }
  }

  int _getGenesisTimestamp(String network) {
    // Genesis timestamps (Unix seconds) for different Monero networks
    switch (network.toLowerCase()) {
      case 'mainnet':
        return 1397818193; // April 18, 2014
      case 'stagenet':
        return 1458748658; // March 23, 2016
      case 'testnet':
        return 1410295020; // September 10, 2014
      default:
        return 1397818193; // Default to mainnet genesis
    }
  }

  void _generateSeed() {
    setState(() {
      _validationError = null;
      _responseError = null;
      _derivedAddress = null;
      _secretSpendKey = null;
      _secretViewKey = null;
      _publicSpendKey = null;
      _publicViewKey = null;
    });

    // Convert UI seed type to backend value
    final seedType = _seedType.contains('polyseed') ? 'polyseed' : 'classic';
    GenerateSeedRequest(seedType: seedType).sendSignalToRust();
  }

  void _deriveAddress() {
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _validationError = null;
        _responseError = null;
        _derivedAddress = null;
        _secretSpendKey = null;
        _secretViewKey = null;
        _publicSpendKey = null;
        _publicViewKey = null;
      });
      return;
    }

    setState(() {
      _validationError = null;
      _responseError = null;
      _derivedAddress = null;
      _secretSpendKey = null;
      _secretViewKey = null;
      _publicSpendKey = null;
      _publicViewKey = null;
    });

    final result = KeyParser.parse(_controller.text);

    if (!result.isValid) {
      setState(() {
        _validationError = result.error;
      });
      return;
    }

    DeriveKeysRequest(
      seed: result.normalizedInput!,
      network: _network,
    ).sendSignalToRust();

    // Also derive subaddresses for the active account
    _deriveSubaddresses();
  }

  void _deriveSubaddresses() {
    if (_controller.text.trim().isEmpty) return;

    final result = KeyParser.parse(_controller.text);
    if (!result.isValid) return;

    if (_activeAccount == -1) {
      // "All" accounts view - derive first 3 unused subaddresses per account
      for (var account in _accounts) {
        final used = _getUsedSubaddresses(account);
        int index = 0;
        int derived = 0;

        while (derived < 3) {
          if (!used.contains(index)) {
            final key = '$account,$index';
            // Only derive if we don't already have it and it's not pending
            if (!_subaddresses.containsKey(key) && !_pendingSubaddresses.contains(key)) {
              _pendingSubaddresses.add(key);
              DeriveSubaddressRequest(
                seed: result.normalizedInput!,
                network: _network,
                account: account,
                addressIndex: index,
              ).sendSignalToRust();
            }
            derived++;
          }
          index++;
        }
      }
    } else {
      // Derive the first 5 unused subaddresses for the active account
      final used = _getUsedSubaddresses(_activeAccount);
      int index = 0;
      int derived = 0;

      while (derived < 5) {
        if (!used.contains(index)) {
          final key = '$_activeAccount,$index';
          // Only derive if we don't already have it and it's not pending
          if (!_subaddresses.containsKey(key) && !_pendingSubaddresses.contains(key)) {
            _pendingSubaddresses.add(key);
            DeriveSubaddressRequest(
              seed: result.normalizedInput!,
              network: _network,
              account: _activeAccount,
              addressIndex: index,
            ).sendSignalToRust();
          }
          derived++;
        }
        index++;
      }
    }
  }

  Set<int> _getUsedSubaddresses(int account) {
    final used = <int>{};
    for (var output in _allOutputsAllAccounts) {
      if (output.subaddressIndex != null) {
        final subIdx = output.subaddressIndex!;
        final outputAccount = subIdx.item1;
        final addressIndex = subIdx.item2;
        if (outputAccount == account) {
          used.add(addressIndex);
        }
      }
    }
    return used;
  }

  void _ensureAccountsExistForOutputs(List<OwnedOutput> outputs) {
    if (_lifecycle.activeWallet == null) return;

    int highestAccountIndex = 0;
    for (var output in outputs) {
      if (output.subaddressIndex != null) {
        final accountIndex = output.subaddressIndex!.item1;
        if (accountIndex > highestAccountIndex) {
          highestAccountIndex = accountIndex;
        }
      }
    }

    var wallet = _lifecycle.activeWallet!;
    bool updated = false;

    for (int i = 0; i <= highestAccountIndex; i++) {
      if (!wallet.accounts.contains(i)) {
        wallet = wallet.createAccount(i);
        updated = true;
      }
    }

    if (updated) {
      _lifecycle.openWallets[wallet.walletId] = wallet;
    }
  }

  void _createAccount() {
    final newAccountIndex = _accounts.isEmpty ? 0 : _accounts.last + 1;

    // Get the active wallet and create a new account in it
    var activeWallet = _lifecycle.activeWallet;

    // If no wallet exists but we have a seed and address, create one first
    if (activeWallet == null && _controller.text.trim().isNotEmpty && _derivedAddress != null) {
      _openWallet(_walletId, _controller.text.trim(), _network, _derivedAddress!);
      activeWallet = _lifecycle.activeWallet;
    }

    // Now check again if we have a wallet
    final wallet = activeWallet;
    if (wallet != null) {
      final updatedWallet = wallet
          .createAccount(newAccountIndex)
          .switchAccount(newAccountIndex);
      setState(() {
        _lifecycle.openWallets[wallet.walletId] = updatedWallet;
      });

      _deriveSubaddresses();

      if (_isContinuousScanning && _controller.text.trim().isNotEmpty) {
        _startContinuousScan();
      }
    }
  }

  void _selectAccount(int accountIndex) {
    final activeWallet = _lifecycle.activeWallet;
    if (activeWallet != null) {
      final updatedWallet = activeWallet.switchAccount(accountIndex);
      setState(() {
        _lifecycle.openWallets[activeWallet.walletId] = updatedWallet;
        // Clear subaddresses cache for other accounts when switching
        // (unless switching to "All" which doesn't need subaddresses)
        if (accountIndex >= 0) {
          final keysToRemove = _subaddresses.keys.where((key) =>
            !key.startsWith('$accountIndex,')).toList();
          for (var key in keysToRemove) {
            _subaddresses.remove(key);
          }
        }
      });

      // Derive subaddresses for the newly selected account (skip for "All")
      if (accountIndex >= 0) {
        _deriveSubaddresses();
      }
    }
  }

  void _toggleAccountScanning(int accountIndex, bool shouldScan) {
    final activeWallet = _lifecycle.activeWallet;
    if (activeWallet != null) {
      final updatedWallet = activeWallet.toggleAccountScanning(accountIndex, shouldScan);
      setState(() {
        _lifecycle.openWallets[activeWallet.walletId] = updatedWallet;
      });
    }
  }

  void _scanBlock() {
    final validation = WalletScanService.validateScanBlock(
      seed: _controller.text,
      blockHeight: _blockHeightController.text,
      nodeUrl: _nodeUrlController.text,
    );

    if (!validation.isValid) {
      setState(() {
        _scanError = validation.error;
      });
      return;
    }

    setState(() {
      _isScanning = true;
      _scanResult = null;
      _scanError = null;
    });

    WalletScanService.scanBlock(
      seed: validation.normalizedSeed!,
      blockHeight: validation.blockHeight!,
      nodeUrl: validation.nodeUrl!,
      network: _network,
    );
  }

  void _startContinuousScan() {
    final walletsToScan = _activeWallets;

    final validation = WalletScanService.validateContinuousScan(
      seed: _controller.text,
      blockHeight: _blockHeightController.text,
      nodeUrl: _nodeUrlController.text,
      activeWallets: walletsToScan,
    );

    if (!validation.isValid) {
      setState(() {
        _scanError = validation.error;
      });
      return;
    }

    // Stop any existing scan to ensure clean transition between scan modes
    final wasScanning = _isContinuousScanning;
    if (wasScanning) {
      WalletScanService.pauseContinuousScan();
      setState(() {
        _isContinuousScanning = false;
      });
    }

    void doStartScan() {
      _stopPollingTimers();

      setState(() {
        _scanError = null;
        _isContinuousPaused = false;
        _isContinuousScanning = true;

        for (var wallet in walletsToScan) {
          wallet.isScanning = true;
        }
      });

      final result = walletsToScan.isEmpty ? KeyParser.parse(_controller.text) : null;
      final highestAccount = _accounts.isEmpty ? 0 : _accounts.reduce((a, b) => a > b ? a : b);
      WalletScanService.startContinuousScan(
        nodeUrl: validation.nodeUrl!,
        startHeight: validation.startHeight!,
        walletsToScan: walletsToScan,
        seed: result?.normalizedInput,
        network: walletsToScan.isEmpty ? _network : null,
        accountLookahead: highestAccount,
      );
    }

    // If we stopped a previous scan, wait for Rust to process the stop
    if (wasScanning) {
      Future.delayed(const Duration(milliseconds: 200), doStartScan);
    } else {
      doStartScan();
    }
  }

  void _pauseContinuousScan() {
    setState(() {
      _isContinuousPaused = true;
      _isContinuousScanning = false;
    });
    WalletScanService.pauseContinuousScan();
    // Start polling timers when scan is paused
    _startPollingTimers();
  }

  void _scanMempool() {
    final validation = WalletScanService.validateMempoolScan(
      seed: _controller.text,
      nodeUrl: _nodeUrlController.text,
    );

    if (!validation.isValid) {
      setState(() {
        _scanError = validation.error;
      });
      return;
    }

    setState(() {
      _isScanningMempool = true;
      _scanError = null;
    });

    WalletScanService.scanMempool(
      seed: validation.normalizedSeed!,
      nodeUrl: validation.nodeUrl!,
      network: _network,
    );
  }

  String _continuousScanButtonLabel() {
    if (_isContinuousScanning) {
      return 'Pause Scan';
    }
    if (_isContinuousPaused) {
      final currentText = _blockHeightController.text.trim();
      final height = int.tryParse(currentText);
      if (_blockHeightUserEdited && height != null && height > _continuousScanCurrentHeight) {
        return 'Start Skipscan';
      }
      return 'Start Rescan';
    }
    return 'Start Scan';
  }

  Color _continuousScanButtonColor() {
    if (_isContinuousScanning) {
      return Colors.orange;
    }
    if (_isContinuousPaused) {
      return Colors.blueGrey;
    }
    return Colors.green;
  }

  String? _checkMultiAccountOutputs() {
    if (_selectedOutputs.isEmpty) {
      return null;
    }

    final selectedAccounts = <int>{};

    for (final outputKey in _selectedOutputs) {
      final output = _allOutputsAllAccounts.where((o) => '${o.txHash}:${o.outputIndex}' == outputKey).firstOrNull;
      if (output != null) {
        final account = output.subaddressIndex?.item1 ?? 0;
        selectedAccounts.add(account);
      }
    }

    if (selectedAccounts.length > 1 && _activeAccount != -1) {
      return 'Cannot create transaction with outputs from multiple accounts (${selectedAccounts.join(', ')}). Switch to "All" accounts view to allow multi-account transactions.';
    }

    return null;
  }

  String? _getMultiAccountWarning() {
    if (_selectedOutputs.isEmpty || _activeAccount != -1) {
      return null;
    }

    final selectedAccounts = <int>{};

    for (final outputKey in _selectedOutputs) {
      final output = _allOutputsAllAccounts.where((o) => '${o.txHash}:${o.outputIndex}' == outputKey).firstOrNull;
      if (output != null) {
        final account = output.subaddressIndex?.item1 ?? 0;
        selectedAccounts.add(account);
      }
    }

    if (selectedAccounts.length > 1) {
      final accountsList = selectedAccounts.toList()..sort();
      return 'WARNING: Creating transaction with outputs from multiple accounts (${accountsList.join(', ')}). This may reduce privacy.';
    }

    return null;
  }

  void _createTransaction() {
    final recipientInputs = List.generate(
      _destinationControllers.length,
      (i) => RecipientInput(
        address: _destinationControllers[i].text,
        amount: _amountControllers[i].text,
      ),
    );

    final multiAccountCheck = _checkMultiAccountOutputs();
    if (multiAccountCheck != null) {
      setState(() {
        _txError = multiAccountCheck;
      });
      return;
    }

    // Check if this is a single-recipient transaction sending the max amount
    final isSingleRecipient = _destinationControllers.length == 1;
    bool isSendingMax = false;

    if (isSingleRecipient) {
      final maxSpendable = TransactionService.calculateMaxSpendable(
        availableOutputs: _allOutputs,
        selectedOutputs: _selectedOutputs.isNotEmpty ? _selectedOutputs : null,
        currentHeight: _currentHeight,
      );

      final amountStr = _amountControllers[0].text.trim();
      final amount = double.tryParse(amountStr);

      // Check if amount equals max (within small tolerance for floating point)
      if (amount != null && (amount - maxSpendable).abs() < 0.000000001) {
        isSendingMax = true;
      }
    }

    // If sending max to a single recipient, use sweepAll
    if (isSendingMax) {
      final destinationAddress = _destinationControllers[0].text;

      // Validate sweep parameters
      final validation = TransactionService.validateSweepAll(
        seed: _controller.text,
        availableOutputs: _allOutputs,
        destinationAddress: destinationAddress,
        nodeUrl: _nodeUrlController.text,
        selectedOutputs: _selectedOutputs.isNotEmpty ? _selectedOutputs : null,
        currentHeight: _currentHeight,
      );

      if (!validation.isValid) {
        setState(() {
          _txError = validation.error;
        });
        return;
      }

      setState(() {
        _isCreatingTx = true;
        _txResult = null;
        _txError = null;
        _broadcastResult = null;
        _broadcastError = null;
      });

      // Execute sweep
      TransactionService.sweepAll(
        seed: validation.normalizedSeed!,
        network: _network,
        destinationAddress: validation.destinationAddress!,
        nodeUrl: validation.nodeUrl!,
        selectedOutputs: validation.selectedOutputs,
      );
    } else {
      // Normal transaction creation
      final validation = TransactionService.validateTransactionCreation(
        seed: _controller.text,
        availableOutputs: _allOutputs,
        recipients: recipientInputs,
        nodeUrl: _nodeUrlController.text,
        selectedOutputs: _selectedOutputs.isNotEmpty ? _selectedOutputs : null,
        currentHeight: _currentHeight,
      );

      if (!validation.isValid) {
        setState(() {
          _txError = validation.error;
        });
        return;
      }

      setState(() {
        _isCreatingTx = true;
        _txResult = null;
        _txError = null;
      });

      // Execute transaction creation
      TransactionService.createTransaction(
        seed: validation.normalizedSeed!,
        network: _network,
        recipients: validation.recipients!,
        nodeUrl: validation.nodeUrl!,
        selectedOutputs: validation.selectedOutputs,
      );
    }
  }

  void _handleSendMax(int recipientIndex) {
    if (_activeAccount == -1) {
      showDialog(
        context: context,
        builder: (context) {
          return AlertDialog(
            title: const Text('Send Max from all accounts?'),
            content: const Text(
              'You are sweeping multiple accounts.\n\nContinue?',
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(),
                child: const Text('Cancel'),
              ),
              ElevatedButton(
                onPressed: () {
                  Navigator.of(context).pop();
                  _setMaxAmount(recipientIndex);
                },
                child: const Text('Continue'),
              ),
            ],
          );
        },
      );
    } else {
      _setMaxAmount(recipientIndex);
    }
  }

  void _setMaxAmount(int recipientIndex) {
    final maxSpendable = TransactionService.calculateMaxSpendable(
      availableOutputs: _allOutputs,
      selectedOutputs: _selectedOutputs.isNotEmpty ? _selectedOutputs : null,
      currentHeight: _currentHeight,
    );

    setState(() {
      _amountControllers[recipientIndex].text = maxSpendable.toStringAsFixed(12);
    });
  }

  void _addRecipient() {
    if (_destinationControllers.length >= 15) return;
    setState(() {
      _destinationControllers.add(TextEditingController());
      _amountControllers.add(TextEditingController());
    });
  }

  void _removeRecipient(int index) {
    if (_destinationControllers.length <= 1) return;
    setState(() {
      _destinationControllers[index].dispose();
      _amountControllers[index].dispose();
      _destinationControllers.removeAt(index);
      _amountControllers.removeAt(index);
    });
  }

  void _broadcastTransaction() {
    // Validate broadcast parameters
    final validation = TransactionService.validateTransactionBroadcast(
      txResult: _txResult,
      nodeUrl: _nodeUrlController.text,
    );

    if (!validation.isValid) {
      setState(() {
        _broadcastError = validation.error;
      });
      return;
    }

    setState(() {
      _isBroadcasting = true;
      _broadcastResult = null;
      _broadcastError = null;
    });

    // Execute transaction broadcast
    TransactionService.broadcastTransaction(
      nodeUrl: validation.nodeUrl!,
      txBlob: validation.txBlob!,
      spentOutputHashes: validation.spentOutputHashes!,
    );
  }

  Future<void> _copyToClipboard(String text, String label) async {
    await ClipboardUtils.copyToClipboard(context, text, label);
  }

  void _navigateToTransaction(String txHash) {
    setState(() {
      // Expand the Transactions panel
      _expandedPanel = DebugPanel.transactions;
      // Expand the specific transaction
      _expandedTransactions.add(txHash);
    });
    // Scroll to the panel (will be visible after setState)
    Future.delayed(const Duration(milliseconds: 100), () {
      // This gives time for the panel to expand before scrolling
      Scrollable.ensureVisible(
        context,
        alignment: 0.5,
        duration: const Duration(milliseconds: 300),
      );
    });
  }

  void _toggleViewMode() {
    _extensionService.isSidePanel
        ? _extensionService.openFullPage()
        : _extensionService.openSidePanel();
  }

  void _showSnackBar(String message, {Color? backgroundColor, int seconds = 2}) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        backgroundColor: backgroundColor,
        duration: Duration(seconds: seconds),
      ),
    );
  }

  void _resetWalletState() {
    _controller.text = '';
    _derivedAddress = null;
    _secretSpendKey = null;
    _secretViewKey = null;
    _publicSpendKey = null;
    _publicViewKey = null;
    _allOutputsAllAccounts = [];
    _allTransactionsAllAccounts = [];
    _expandedTransactions = {};
    _selectedOutputs = {};
    _continuousScanCurrentHeight = 0;
    _continuousScanTargetHeight = 0;
    _isSynced = false;
    _daemonHeight = null;
    _scanResult = null;
    _scanError = null;
    _lastSaveTime = null;
    _loadError = null;
    _saveError = null;
  }

  ExpansionPanel _buildPanel({
    required DebugPanel panel,
    String? subtitle,
    required Widget body,
  }) {
    return ExpansionPanel(
      headerBuilder: (BuildContext context, bool isExpanded) {
        return GestureDetector(
          onTap: () {
            setState(() {
              _expandedPanel = (_expandedPanel == panel) ? null : panel;
            });
          },
          child: ListTile(
            title: Text(
              panel.title,
              style: const TextStyle(fontWeight: FontWeight.bold),
            ),
            subtitle: subtitle != null
                ? Text(subtitle, style: const TextStyle(fontSize: 12))
                : null,
          ),
        );
      },
      body: body,
      isExpanded: _expandedPanel == panel,
    );
  }

  @override
  Widget build(BuildContext context) {
    final isSidePanel = _extensionService.isSidePanel;

    // Pre-compute dynamic subtitles
    final hasData = WalletPersistenceBrowser.hasWalletData(_walletId);
    final totalBytes = _calculateTotalStorageBytes();
    final fileManagementSubtitle = hasData
        ? 'Data stored: ${_formatBytes(totalBytes)}'
        : 'No stored data';

    final txCount = _allTransactions.length;
    final incomingCount = _allTransactions.where((t) => t.isIncoming(_allOutputs)).length;
    final outgoingCount = txCount - incomingCount;
    final transactionsSubtitle = txCount == 0
        ? 'No transactions'
        : '$txCount transaction${txCount == 1 ? '' : 's'} ($incomingCount in, $outgoingCount out)';

    final balance = BalanceUtils.calculate(_allOutputs, _currentHeight, _selectedOutputs);
    final coinsSubtitle = '${balance.balanceStr} - ${balance.outputCountStr}${balance.selectedStr}';

    return Scaffold(
      appBar: AppBar(
        title: const Text('Debug View'),
        actions: [
          if (_extensionService.isExtension)
            IconButton(
              icon: Icon(isSidePanel ? Icons.open_in_full : Icons.close_fullscreen),
              tooltip: isSidePanel ? 'Expand to Page' : 'Minimize to Side Panel',
              onPressed: _toggleViewMode,
            ),
        ],
      ),
      body: SingleChildScrollView(
        child: Padding(
          padding: const EdgeInsets.all(16.0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              ExpansionPanelList(
                  expansionCallback: (int index, bool isExpanded) {
                    setState(() {
                      final panel = DebugPanel.fromIndex(index);
                      _expandedPanel = (_expandedPanel == panel) ? null : panel;
                    });
                  },
                  expandIconColor: Theme.of(context).colorScheme.primary,
                  elevation: 1,
                  expandedHeaderPadding: EdgeInsets.zero,
                  children: [
                    _buildPanel(
                      panel: DebugPanel.fileManagement,
                      subtitle: fileManagementSubtitle,
                      body: FileManagementPanel(
                        walletId: _walletId,
                        availableWalletIds: _availableWalletIds,
                        activeWallets: _activeWallets,
                        activeWalletId: _activeWalletId,
                        lastSaveTime: _lastSaveTime,
                        isSaving: _isSaving,
                        isLoadingWallet: _isLoadingWallet,
                        isExporting: _isExporting,
                        isImporting: _isImporting,
                        saveError: _saveError,
                        loadError: _loadError,
                        exportError: _exportError,
                        importError: _importError,
                        seed: _controller.text.trim(),
                        transactionCount: _allTransactionsAllAccounts.length,
                        outputCount: _allOutputsAllAccounts.length,
                        onWalletChanged: _switchWallet,
                        onLoad: _loadWalletData,
                        onDelete: _clearStoredData,
                        onSave: _saveWalletData,
                        onNew: _startNewWallet,
                        onExport: _exportWallet,
                        onImport: _importWallet,
                        onCloseWallet: _closeWallet,
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.seedPhrase,
                      body: SeedPhrasePanel(
                        controller: _controller,
                        seedType: _seedType,
                        network: _network,
                        validationError: _validationError,
                        responseError: _responseError,
                        onGenerateSeed: _generateSeed,
                        onNetworkChanged: (value) {
                          setState(() {
                            _network = value;
                          });
                          _deriveAddress();
                        },
                        onSeedTypeChanged: (value) {
                          setState(() {
                            _seedType = value;
                          });
                        },
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.keys,
                      body: KeysDisplayPanel(
                        address: _derivedAddress,
                        secretSpendKey: _secretSpendKey,
                        secretViewKey: _secretViewKey,
                        publicSpendKey: _publicSpendKey,
                        publicViewKey: _publicViewKey,
                        network: _network,
                        onCopyToClipboard: _copyToClipboard,
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.receive,
                      body: ReceivePanel(
                        seed: _controller.text.trim().isEmpty ? null : _controller.text,
                        network: _network,
                        activeAccount: _activeAccount,
                        accounts: _accounts,
                        allOutputs: _allOutputs,
                        onAccountSelected: _selectAccount,
                        onCreateAccount: _createAccount,
                        onCopyToClipboard: _copyToClipboard,
                        subaddresses: _subaddresses,
                        onNavigateToTransaction: _navigateToTransaction,
                        scanningAccounts: _lifecycle.activeWallet?.scanningAccounts ?? {0},
                        onScanToggle: _toggleAccountScanning,
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.scanning,
                      body: ScanningPanel(
                        nodeUrlController: _nodeUrlController,
                        blockHeightController: _blockHeightController,
                        blockHeightFocusNode: _blockHeightFocusNode,
                        isScanning: _isScanning,
                        isContinuousScanning: _isContinuousScanning,
                        isContinuousPaused: _isContinuousPaused,
                        isSynced: _isSynced,
                        isScanningMempool: _isScanningMempool,
                        continuousScanCurrentHeight: _continuousScanCurrentHeight,
                        continuousScanTargetHeight: _continuousScanTargetHeight,
                        scanError: _scanError,
                        scanResult: _scanResult,
                        hasSeedPhrase: _controller.text.trim().isNotEmpty,
                        pollingService: _pollingService,
                        onScanBlock: _scanBlock,
                        onStartContinuousScan: _startContinuousScan,
                        onPauseContinuousScan: _pauseContinuousScan,
                        onScanMempool: _scanMempool,
                        getContinuousScanButtonLabel: _continuousScanButtonLabel,
                        getContinuousScanButtonColor: _continuousScanButtonColor,
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.transactions,
                      subtitle: transactionsSubtitle,
                      body: TransactionsPanel(
                        allTransactions: _sortedTransactions(),
                        allOutputs: _allOutputsAllAccounts,
                        currentHeight: _currentHeight,
                        txSortBy: _txSortBy,
                        txSortAscending: _txSortAscending,
                        expandedTransactions: _expandedTransactions,
                        activeAccount: _activeAccount,
                        onSortChanged: (sortKey) {
                          setState(() {
                            if (_txSortBy == sortKey) {
                              _txSortAscending = !_txSortAscending;
                            } else {
                              _txSortBy = sortKey;
                              _txSortAscending = false;
                            }
                          });
                        },
                        onToggleExpanded: (txHash) {
                          setState(() {
                            if (_expandedTransactions.contains(txHash)) {
                              _expandedTransactions.remove(txHash);
                            } else {
                              _expandedTransactions.add(txHash);
                            }
                          });
                        },
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.coins,
                      subtitle: coinsSubtitle,
                      body: OutputsPanel(
                        allOutputs: _allOutputs,
                        currentHeight: _currentHeight,
                        showSpentOutputs: _showSpentOutputs,
                        sortBy: _sortBy,
                        sortAscending: _sortAscending,
                        selectedOutputs: _selectedOutputs,
                        activeAccount: _activeAccount,
                        onToggleShowSpent: () {
                          setState(() {
                            _showSpentOutputs = !_showSpentOutputs;
                          });
                        },
                        onSelectAllSpendable: _selectAllSpendable,
                        onClearSelection: _clearSelection,
                        onSortChanged: (sortKey) {
                          setState(() {
                            if (_sortBy == sortKey) {
                              _sortAscending = !_sortAscending;
                            } else {
                              _sortBy = sortKey;
                              _sortAscending = false;
                            }
                          });
                        },
                        onOutputSelectionChanged: (outputKey, selected) {
                          setState(() {
                            if (selected) {
                              _selectedOutputs.add(outputKey);
                            } else {
                              _selectedOutputs.remove(outputKey);
                            }
                          });
                        },
                      ),
                    ),
                    _buildPanel(
                      panel: DebugPanel.createTransaction,
                      body: CreateTransactionPanel(
                        destinationControllers: _destinationControllers,
                        amountControllers: _amountControllers,
                        isCreatingTx: _isCreatingTx,
                        isBroadcasting: _isBroadcasting,
                        txResult: _txResult,
                        broadcastResult: _broadcastResult,
                        txError: _txError,
                        broadcastError: _broadcastError,
                        multiAccountWarning: _getMultiAccountWarning(),
                        onAddRecipient: _addRecipient,
                        onRemoveRecipient: _removeRecipient,
                        onCreateTransaction: _createTransaction,
                        onBroadcastTransaction: _broadcastTransaction,
                        onProvePayment: () => PaymentProofDialog.show(
                          context,
                          txResult: _txResult,
                          destinationControllers: _destinationControllers,
                          network: _network,
                        ),
                        onAmountChanged: () => setState(() {}),
                        onSendMax: _handleSendMax,
                      ),
                    ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }

  // Helper methods for storage size calculation
  int _calculateTotalStorageBytes() {
    int totalBytes = 0;
    for (var i = 0; i < html.window.localStorage.length; i++) {
      final key = html.window.localStorage.keys.elementAt(i);
      if (key.startsWith('monero_wallet_')) {
        final value = html.window.localStorage[key];
        if (value != null) {
          totalBytes += value.length;
        }
      }
    }
    return totalBytes;
  }

  String _formatBytes(int bytes) {
    if (bytes >= 1048576) {
      // 1 MiB = 1024 * 1024
      return '${(bytes / 1048576).toStringAsFixed(2)} MiB';
    } else if (bytes >= 1024) {
      // 1 KiB
      return '${(bytes / 1024).toStringAsFixed(2)} KiB';
    } else {
      return '$bytes bytes';
    }
  }

  List<WalletTransaction> _sortedTransactions() {
    return TransactionUtils.sortTransactions(
      _allTransactions,
      _allOutputsAllAccounts,
      _txSortBy,
      _txSortAscending,
      _currentHeight,
    );
  }

  void _selectAllSpendable() {
    setState(() {
      _selectedOutputs = OutputUtils.selectAllSpendable(_allOutputs, _currentHeight);
    });
  }

  void _clearSelection() {
    setState(() {
      _selectedOutputs.clear();
    });
  }

  Future<void> _saveWalletData() async {
    setState(() {
      _isSaving = true;
      _saveError = null;
    });

    // Show save dialog with wallet ID and password
    final result = await showDialog<Map<String, String>>(
      context: context,
      barrierDismissible: false,
      builder: (context) => SaveWalletDialog(
        initialWalletId: _walletId.isEmpty ? 'my_wallet' : _walletId,
        existingWalletIds: _availableWalletIds,
      ),
    );

    if (result == null) {
      setState(() {
        _isSaving = false;
      });
      return;
    }

    final walletId = result['walletId']!;
    final password = result['password']!;

    // Update the current wallet ID to the one being saved
    setState(() {
      _walletId = walletId;
    });

    // Warn if password is empty
    if (password.isEmpty) {
      final proceed = await SecurityWarningDialog.show(context);
      if (proceed != true) {
        setState(() {
          _isSaving = false;
        });
        return;
      }
    }

    final activeWallet = _lifecycle.activeWallet;

    // Derive accounts from outputs if no active wallet exists
    final Set<int> derivedAccounts = {0}; // Always include account 0
    for (var output in _allOutputs) {
      if (output.subaddressIndex != null) {
        derivedAccounts.add(output.subaddressIndex!.item1);
      }
    }

    // Build outputsByAccount from _allOutputs if needed
    final Map<int, List<OwnedOutput>> derivedOutputsByAccount = {};
    for (var output in _allOutputs) {
      final account = output.subaddressIndex?.item1 ?? 0;
      derivedOutputsByAccount.putIfAbsent(account, () => []).add(output);
    }

    final accounts = activeWallet?.accounts ?? derivedAccounts.toList()..sort();
    final outputsByAccount = activeWallet?.outputsByAccount ?? derivedOutputsByAccount;
    final activeAccount = activeWallet?.activeAccount ?? _activeAccount;
    final scanningAccounts = activeWallet?.scanningAccounts ?? derivedAccounts;

    final saveResult = await WalletPersistenceBrowser.saveWalletData(
      walletId: walletId,
      password: password,
      seed: _controller.text.trim(),
      network: _network,
      address: _derivedAddress,
      nodeUrl: _nodeUrlController.text,
      outputs: _allOutputs,
      transactions: _allTransactions,
      continuousScanCurrentHeight: _continuousScanCurrentHeight,
      selectedOutputs: _selectedOutputs,
      accounts: accounts,
      outputsByAccount: outputsByAccount,
      activeAccount: activeAccount,
      scanningAccounts: scanningAccounts,
    );

    final success = saveResult.success;

    setState(() {
      _isSaving = false;
      if (success) {
        _saveError = null;
        _lastSaveTime = DateTime.now().toString().substring(0, 19);
      } else {
        _saveError = saveResult.error ?? 'Failed to save wallet data';
      }
    });

    // Refresh wallet list to include the newly saved wallet
    if (success) {
      _refreshAvailableWallets();
    }

    if (success) {
      _showSnackBar('Wallet "$walletId" saved successfully');
    } else {
      _showSnackBar('Save failed: $_saveError', backgroundColor: Colors.red, seconds: 3);
    }
  }

  void _refreshAvailableWallets() {
    setState(() {
      _lifecycle.refreshAvailableWallets();
    });
  }

  void _startNewWallet() {
    _stopPollingTimers();

    setState(() {
      _lifecycle.startNewWallet();
      _resetWalletState();
      _isContinuousScanning = false;
      _isContinuousPaused = false;
      _txResult = null;
      _txError = null;
      _broadcastResult = null;
      _broadcastError = null;
    });

    _showSnackBar('Ready for new wallet - generate or enter a seed phrase');
  }

  Future<void> _switchWallet(String newWalletId) async {
    final result = _lifecycle.switchWallet(newWalletId);

    switch (result) {
      case SwitchResult.alreadyCurrent:

        return;
      case SwitchResult.switchedToOpen:
        final wallet = _lifecycle.activeWallet!;
        _isRestoringWallet = true;
        setState(() {
          _controller.text = wallet.seed;
          _network = wallet.network;
          _derivedAddress = wallet.address;
          _daemonHeight = wallet.daemonHeight;
        });
        _isRestoringWallet = false;
        return;
      case SwitchResult.needsLoad:
        await _loadWalletData();
        return;
      case SwitchResult.reset:
        setState(() {
          _resetWalletState();
        });
        return;
    }
  }

  void _openWallet(String walletId, String seed, String network, String address, {
    List<int>? accounts,
    Map<int, List<OwnedOutput>>? outputsByAccount,
    int? activeAccount,
  }) {
    setState(() {
      final wallet = _lifecycle.openWallet(walletId, seed, network, address);

      if (accounts != null && accounts.isNotEmpty) {
        final updatedWallet = wallet.copyWith(
          accounts: accounts,
          outputsByAccount: outputsByAccount ?? {},
          activeAccount: activeAccount ?? 0,
        );
        _lifecycle.openWallets[walletId] = updatedWallet;
        // CRITICAL: Ensure activeWalletId is still set after replacing the wallet
        _lifecycle.activeWalletId = walletId;
      }

      _derivedAddress = address;
    });

    _updateBlockHeightFromWallets();
  }

  void _updateBlockHeightFromWallets() {
    if (_activeWallets.isNotEmpty) {
      final lowestHeight = _lowestSyncedHeight;
      if (lowestHeight > 0 && !_blockHeightUserEdited) {
        setState(() {
          _blockHeightController.text = lowestHeight.toString();
        });
      }
    }
  }

  Future<void> _closeWallet(String walletId) async {
    if (_openWallets[walletId] == null) return;

    final shouldSave = await CloseWalletDialog.show(context, walletId);
    if (shouldSave == null) return;

    if (shouldSave) {
      final previousWalletId = _activeWalletId;
      _switchToWallet(walletId);
      await _saveWalletData();
      if (previousWalletId != null && previousWalletId != walletId) {
        _switchToWallet(previousWalletId);
      }
    }

    final result = _lifecycle.closeWallet(walletId);
    if (!result.found) return;

    if (result.switchedTo != null) {
      _isRestoringWallet = true;
      setState(() {
        _controller.text = result.switchedTo!.seed;
        _network = result.switchedTo!.network;
        _derivedAddress = result.switchedTo!.address;
        _daemonHeight = result.switchedTo!.daemonHeight;
      });
      _isRestoringWallet = false;
    } else {
      setState(() {});
    }

    _updateBlockHeightFromWallets();

    if (_isContinuousScanning && _activeWallets.isNotEmpty) {
      _pauseContinuousScan();
      Future.delayed(const Duration(milliseconds: 500), () {
        _startContinuousScan();
      });
    }
  }

  void _switchToWallet(String walletId) {
    final wallet = _lifecycle.switchToWallet(walletId);
    if (wallet == null) return;

    _isRestoringWallet = true;
    setState(() {
      _controller.text = wallet.seed;
      _network = wallet.network;
      _derivedAddress = wallet.address;
      _daemonHeight = wallet.daemonHeight;
    });
    _isRestoringWallet = false;
  }


  Future<void> _loadWalletData() async {
    setState(() {
      _isLoadingWallet = true;
      _loadError = null;
    });

    // Show password dialog
    final password = await showDialog<String>(
      context: context,
      barrierDismissible: false,
      builder: (context) => PasswordDialog(
        isUnlock: true,
        title: 'Unlock Wallet Data',
        submitLabel: 'Unlock',
      ),
    );

    if (password == null) {
      setState(() {
        _isLoadingWallet = false;
      });
      return;
    }

    // Use the persistence service to load wallet data
    final loadResult = await WalletPersistenceBrowser.loadWalletData(
      walletId: _walletId,
      password: password,
    );

    if (!loadResult.success) {
      setState(() {
        _isLoadingWallet = false;
        _loadError = loadResult.error ?? 'Failed to load wallet data';
      });
      return;
    }

    // Capture loaded data before any state changes can wipe it
    final seed = loadResult.seed!;
    final network = loadResult.network!;
    final address = loadResult.address;
    final loadedOutputs = loadResult.outputs!;
    final loadedTransactions = loadResult.transactions!;
    final loadedHeight = loadResult.continuousScanCurrentHeight!;
    final loadedSelectedOutputs = loadResult.selectedOutputs!;
    final loadedAccounts = loadResult.accounts ?? [0];
    final loadedOutputsByAccount = loadResult.outputsByAccount ?? {0: loadedOutputs};
    final loadedActiveAccount = loadResult.activeAccount ?? 0;

    // Restore wallet state (flag prevents _onSeedChanged from wiping data)
    _isRestoringWallet = true;
    setState(() {
      _controller.text = seed;
      _network = network;
      _derivedAddress = address;
      _nodeUrlController.text = loadResult.nodeUrl!;
      _continuousScanCurrentHeight = loadedHeight;
      _continuousScanTargetHeight = 0;
      _isSynced = false;
      _daemonHeight = null;
      _isContinuousScanning = false;
      _isContinuousPaused = loadedHeight > 0;

      if (loadedHeight > 0) {
        _blockHeightController.text = loadedHeight.toString();
        _blockHeightUserEdited = false;
      }

      _isLoadingWallet = false;
      _loadError = null;

      // Open the wallet INSIDE setState to prevent race condition
      // This ensures activeWalletId is set before any scan results arrive
      final resolvedAddress = address ?? _derivedAddress ?? '';
      if (seed.isNotEmpty && resolvedAddress.isNotEmpty) {
        // walletId was passed to _loadWalletData, use it
        _openWallet(
          _walletId,
          seed,
          network,
          resolvedAddress,
          accounts: loadedAccounts,
          outputsByAccount: loadedOutputsByAccount,
          activeAccount: 0,  // Always start with account 0 when loading
        );
      }
    });
    _isRestoringWallet = false;

    setState(() {
      _lifecycle.restoreLoadedData(
        outputs: loadedOutputs,
        transactions: loadedTransactions,
        selectedOutputs: loadedSelectedOutputs,
        scanHeight: loadedHeight,
        daemonHeight: _daemonHeight ?? 0,
      );
    });
    
    // Hydrate Rust WalletActor with restored outputs so transactions work
    if (loadedOutputs.isNotEmpty) {
      RestoreWalletDataRequest(
        seed: seed,
        network: network,
        outputs: loadedOutputs,
        daemonHeight: Uint64(BigInt.from(_daemonHeight ?? 0)),
        currentHeight: Uint64(BigInt.from(loadedHeight)),
      ).sendSignalToRust();
    }

    // Derive address to populate keys
    _deriveAddress();

    _showSnackBar('Wallet data loaded successfully');
  }

  Future<void> _exportWallet() async {
    // Validation
    if (_walletId.isEmpty) {
      setState(() {
        _exportError = 'No wallet selected for export';
      });
      _showSnackBar('Please select or save a wallet first', backgroundColor: Colors.orange);
      return;
    }

    // Check if wallet has saved data
    if (!WalletPersistenceBrowser.hasWalletData(_walletId)) {
      setState(() {
        _exportError = 'No saved data found for this wallet';
      });
      _showSnackBar('Please save wallet data before exporting', backgroundColor: Colors.orange);
      return;
    }

    setState(() {
      _isExporting = true;
      _exportError = null;
    });

    // Use the persistence service to export wallet
    final exportResult = await WalletPersistenceBrowser.exportWallet(
      walletId: _walletId,
    );

    setState(() {
      _isExporting = false;
      if (!exportResult.success) {
        _exportError = exportResult.error;
      } else {
        _exportError = null;
      }
    });

    if (exportResult.cancelled) {
      // User cancelled, do nothing
      return;
    }

    if (exportResult.success) {
      final msg = exportResult.usedSaveAsDialog!
          ? 'Wallet "$_walletId" saved'
          : 'Wallet "$_walletId" exported as ${exportResult.filename}';
      _showSnackBar(msg, seconds: 3);
    } else {
      _showSnackBar('Export failed: ${exportResult.error}', backgroundColor: Colors.red, seconds: 4);
    }
  }

  Future<void> _importWallet() async {
    // Don't set _isImporting here - wait until file is selected
    setState(() {
      _importError = null;
    });

    try {
      // Create file input element (WASM-compatible)
      final uploadInput = html.FileUploadInputElement();
      uploadInput.accept = '.monero-wallet,*'; // Prefer .monero-wallet, allow all as fallback
      uploadInput.click();

      // Wait for file selection - this may never complete if cancelled
      // So we wrap in a timeout
      try {
        await uploadInput.onChange.first.timeout(
          const Duration(seconds: 120),
        );
      } on TimeoutException {
        // Dialog was likely cancelled
        return;
      }

      final files = uploadInput.files;
      if (files == null || files.isEmpty) {
        // No file selected (shouldn't happen but be safe)
        return;
      }

      // Now we know a file was selected, show loading state
      setState(() {
        _isImporting = true;
      });

      final file = files[0];

      // Extract suggested wallet ID from filename
      final suggestedWalletId = WalletPersistenceBrowser.extractWalletIdFromFilename(file.name);

      // Prompt for wallet ID (loop until valid non-conflicting ID or user cancels)
      String? walletId;
      bool shouldOverwrite = false;

      while (true) {
        if (!mounted) return;

        walletId = await showDialog<String>(
          context: context,
          barrierDismissible: false,
          builder: (context) => WalletIdDialog(
            suggestedWalletId: suggestedWalletId,
            existingWalletIds: _availableWalletIds,
          ),
        );

        if (walletId == null || walletId.isEmpty) {
          setState(() {
            _isImporting = false;
          });
          return;
        }

        // Check if wallet ID already exists
        if (_availableWalletIds.contains(walletId)) {
          if (!mounted) return;

          // Show overwrite confirmation dialog
          final result = await OverwriteWalletDialog.show(context, walletId);

          if (result == 'cancel') {
            setState(() {
              _isImporting = false;
            });
            return;
          } else if (result == 'choose_different') {
            // Loop back to prompt for a different wallet ID
            continue;
          } else if (result == 'overwrite') {
            shouldOverwrite = true;
            break; // Exit loop and proceed with import
          }
        } else {
          // Wallet ID doesn't exist, proceed with import
          break;
        }
      }

      // Prompt for password to verify decryption
      if (!mounted) return;
      final password = await showDialog<String>(
        context: context,
        barrierDismissible: false,
        builder: (context) => PasswordDialog(
          isUnlock: true,
          title: 'Verify Wallet Password',
          submitLabel: 'Import',
        ),
      );

      if (password == null) {
        setState(() {
          _isImporting = false;
        });
        return;
      }

      // Stop any active scans before importing to prevent race condition
      if (_isContinuousScanning) {
        WalletScanService.pauseContinuousScan();
        setState(() {
          _isContinuousScanning = false;
          _isContinuousPaused = false;
        });
      }

      // Use the persistence service to import wallet
      final importResult = await WalletPersistenceBrowser.importWallet(
        file: file,
        walletId: walletId,
        password: password,
        shouldOverwrite: shouldOverwrite,
      );

      setState(() {
        _isImporting = false;
        if (!importResult.success) {
          _importError = importResult.error;
        } else {
          _importError = null;
        }
      });

      if (importResult.success) {
        // Switch to imported wallet
        await _switchWallet(walletId);
        
        final msg = shouldOverwrite
            ? 'Wallet "$walletId" overwritten successfully'
            : 'Wallet "$walletId" imported successfully';
        _showSnackBar(msg, seconds: 3);
      } else {
        _showSnackBar('Import failed: ${importResult.error}', backgroundColor: Colors.red, seconds: 4);
      }
    } catch (e) {
      setState(() {
        _isImporting = false;
        _importError = 'Import failed: $e';
      });

      _showSnackBar('Import failed: $e', backgroundColor: Colors.red, seconds: 4);
    }
  }

  Future<void> _clearStoredData() async {
    final confirmed = await DeleteConfirmationDialog.show(context, _walletId);
    if (confirmed != true) return;

    final deletedWalletId = _walletId;
    WalletPersistenceBrowser.clearWalletData(deletedWalletId);
    _refreshAvailableWallets();
    _startNewWallet();
    _showSnackBar('Deleted wallet: $deletedWalletId');
  }
}

