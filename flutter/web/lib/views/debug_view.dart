import 'dart:async';
import 'dart:html' as html;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';
import '../services/extension_service.dart';
import '../widgets/password_dialog.dart';
import '../widgets/wallet_id_dialog.dart';
import '../widgets/save_wallet_dialog.dart';
import '../services/wallet_persistence_service.dart';
import '../models/wallet_instance.dart';
import '../models/wallet_transaction.dart';
import '../utils/clipboard_utils.dart';
import '../utils/output_utils.dart';
import '../utils/transaction_utils.dart';
import '../services/wallet_scan_service.dart';
import '../services/transaction_service.dart';
import '../services/wallet_polling_service.dart';
import '../widgets/common_widgets.dart';
import '../widgets/keys_display_panel.dart';
import '../widgets/scanning_panel.dart';
import '../widgets/transactions_panel.dart';
import '../widgets/outputs_panel.dart';
import '../widgets/seed_phrase_panel.dart';
import '../widgets/file_management_panel.dart';
import '../widgets/create_transaction_panel.dart';
import '../widgets/overwrite_wallet_dialog.dart';

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

  // Current wallet ID (multi-wallet support)
  String _walletId = '';
  List<String> _availableWalletIds = [];

  // Multi-wallet instances (wallets currently open/scanning)
  Map<String, WalletInstance> _openWallets = {};
  String? _activeWalletId; // Currently displayed wallet

  WalletInstance? get _activeWallet =>
      _activeWalletId != null ? _openWallets[_activeWalletId] : null;

  List<WalletInstance> get _activeWallets =>
      _openWallets.values.where((w) => !w.isClosed).toList();

  int get _lowestSyncedHeight {
    final heights = _activeWallets
        .where((w) => w.currentHeight > 0)
        .map((w) => w.currentHeight)
        .toList();
    return heights.isEmpty ? 0 : heights.reduce((a, b) => a < b ? a : b);
  }

  String _network = 'stagenet';
  final String _seedType = '25 word';
  String? _validationError;
  String? _derivedAddress;
  String? _responseError;
  bool _isScanning = false;
  Timer? _debounceTimer;
  String? _secretSpendKey;
  String? _secretViewKey;
  String? _publicSpendKey;
  String? _publicViewKey;

  BlockScanResponse? _scanResult;
  String? _scanError;
  List<OwnedOutput> _allOutputs = [];
  int? _daemonHeight;

  // Transaction tracking state
  List<WalletTransaction> _allTransactions = [];
  String _txSortBy = 'confirms'; // 'confirms' or 'amount'
  bool _txSortAscending = false;
  Set<String> _expandedTransactions = {}; // Track which transaction cards are expanded

  // Current blockchain height (defaults to 0)
  int get _currentHeight => _daemonHeight ?? _scanResult?.blockHeight.toInt() ?? 0;

  // Continuous scan state
  bool _isContinuousScanning = false;
  bool _isContinuousPaused = false;
  int _continuousScanCurrentHeight = 0;
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
  Set<String> _selectedOutputs = {}; // "txHash:outputIndex" keys for coin control

  bool _isScanningMempool = false;

  // File management state
  bool _isSaving = false;
  bool _isLoadingWallet = false;
  String? _saveError;
  String? _loadError;
  String? _lastSaveTime;
  bool _isExporting = false;
  bool _isImporting = false;
  String? _exportError;
  String? _importError;

  // Helper to get storage key for current wallet
  String get _storageKey => WalletPersistenceService.getStorageKey(_walletId);

  int? _expandedPanel;

  // Stream subscriptions
  StreamSubscription? _keysDerivedSubscription;
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

    _seedGeneratedSubscription = SeedGeneratedResponse.rustSignalStream.listen((signal) {
      if (signal.message.success) {
        setState(() {
          _controller.text = signal.message.seed;
          _validationError = null;
          _responseError = null;
          _derivedAddress = null;
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
          // Add new outputs or update unconfirmed ones that are now confirmed
          for (var output in signal.message.outputs) {
            final existingIndex = _allOutputs.indexWhere((o) =>
              o.txHash == output.txHash && o.outputIndex == output.outputIndex
            );
            if (existingIndex == -1) {
              _allOutputs.add(output);
            } else if (_allOutputs[existingIndex].blockHeight.toInt() == 0) {
              // Update unconfirmed output with confirmed block height
              _allOutputs[existingIndex] = output;
            }
          }

          // Track transactions: group outputs by txHash and track spent key images
          _updateTransactionsFromScan(signal.message);
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

          // Add change outputs to the outputs list as locked (0 block height = unconfirmed)
          for (var changeOutput in signal.message.changeOutputs) {
            final exists = _allOutputs.any((o) =>
              o.txHash == changeOutput.txHash && o.outputIndex == changeOutput.outputIndex
            );
            if (!exists) {
              _allOutputs.add(OwnedOutput(
                txHash: changeOutput.txHash,
                outputIndex: changeOutput.outputIndex,
                amount: changeOutput.amount,
                amountXmr: changeOutput.amountXmr,
                key: changeOutput.key,
                keyOffset: changeOutput.keyOffset,
                commitmentMask: changeOutput.commitmentMask,
                subaddressIndex: changeOutput.subaddressIndex,
                paymentId: null,
                receivedOutputBytes: changeOutput.receivedOutputBytes,
                blockHeight: Uint64(BigInt.zero), // Unconfirmed - will be updated when mined
                spent: false,
                keyImage: changeOutput.keyImage,
              ));
            }
          }
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
            for (var outputKey in _txResult!.spentOutputHashes) {
              _selectedOutputs.remove(outputKey);
              // Find and mark the output as spent
              for (int i = 0; i < _allOutputs.length; i++) {
                final output = _allOutputs[i];
                final key = '${output.txHash}:${output.outputIndex}';
                if (key == outputKey) {
                  _allOutputs[i] = OwnedOutput(
                    txHash: output.txHash,
                    outputIndex: output.outputIndex,
                    amount: output.amount,
                    amountXmr: output.amountXmr,
                    key: output.key,
                    keyOffset: output.keyOffset,
                    commitmentMask: output.commitmentMask,
                    subaddressIndex: output.subaddressIndex,
                    paymentId: output.paymentId,
                    receivedOutputBytes: output.receivedOutputBytes,
                    blockHeight: output.blockHeight,
                    spent: true,
                    keyImage: output.keyImage,
                  );
                  break;
                }
              }
            }
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

      // Stop polling timers when scanning starts
      if (_isContinuousScanning && !wasScanning) {
        _stopPollingTimers();
      }
      // Start polling timers when sync completes (or scan finishes while synced)
      if (_isSynced && !_isContinuousScanning && (wasScanning || !wasSynced)) {
        _startPollingTimers();
      }
    });

    _spentStatusUpdatedSubscription = SpentStatusUpdatedResponse.rustSignalStream.listen((signal) {
      setState(() {
        for (var keyImage in signal.message.spentKeyImages) {
          for (var output in _allOutputs) {
            if (output.keyImage == keyImage) {
              final index = _allOutputs.indexOf(output);
              // Remove from selected outputs since it's now spent
              final outputKey = '${output.txHash}:${output.outputIndex}';
              _selectedOutputs.remove(outputKey);
              _allOutputs[index] = OwnedOutput(
                txHash: output.txHash,
                outputIndex: output.outputIndex,
                amount: output.amount,
                amountXmr: output.amountXmr,
                key: output.key,
                keyOffset: output.keyOffset,
                commitmentMask: output.commitmentMask,
                subaddressIndex: output.subaddressIndex,
                paymentId: output.paymentId,
                receivedOutputBytes: output.receivedOutputBytes,
                blockHeight: output.blockHeight,
                spent: true,
                keyImage: output.keyImage,
              );
            }
          }
        }
      });
    });

    _mempoolScanSubscription = MempoolScanResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isScanningMempool = false;
        if (signal.message.success) {
          // Add new unconfirmed outputs (with block_height 0), avoiding duplicates
          for (var output in signal.message.outputs) {
            final exists = _allOutputs.any((o) =>
              o.txHash == output.txHash && o.outputIndex == output.outputIndex
            );
            if (!exists) {
              // Outputs from mempool have block_height 0 (unconfirmed)
              _allOutputs.add(OwnedOutput(
                txHash: output.txHash,
                outputIndex: output.outputIndex,
                amount: output.amount,
                amountXmr: output.amountXmr,
                key: output.key,
                keyOffset: output.keyOffset,
                commitmentMask: output.commitmentMask,
                subaddressIndex: output.subaddressIndex,
                paymentId: output.paymentId,
                receivedOutputBytes: output.receivedOutputBytes,
                blockHeight: Uint64(BigInt.zero),
                spent: output.spent,
                keyImage: output.keyImage,
              ));
            }
          }
          // Update spent status based on spent_key_images
          for (var keyImage in signal.message.spentKeyImages) {
            for (int i = 0; i < _allOutputs.length; i++) {
              final output = _allOutputs[i];
              if (output.keyImage == keyImage && !output.spent) {
                final outputKey = '${output.txHash}:${output.outputIndex}';
                _selectedOutputs.remove(outputKey);
                _allOutputs[i] = OwnedOutput(
                  txHash: output.txHash,
                  outputIndex: output.outputIndex,
                  amount: output.amount,
                  amountXmr: output.amountXmr,
                  key: output.key,
                  keyOffset: output.keyOffset,
                  commitmentMask: output.commitmentMask,
                  subaddressIndex: output.subaddressIndex,
                  paymentId: output.paymentId,
                  receivedOutputBytes: output.receivedOutputBytes,
                  blockHeight: output.blockHeight,
                  spent: true,
                  keyImage: output.keyImage,
                );
              }
            }
          }
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
        // Update daemon height
        _daemonHeight = response.daemonHeight.toInt();

        // Distribute outputs to corresponding wallets
        for (var walletResult in response.walletResults) {
          final walletInstance = _openWallets.values.cast<WalletInstance?>().firstWhere(
            (w) => w != null && w.address == walletResult.address,
            orElse: () => null,
          );

          if (walletInstance != null) {
            // Add new outputs (avoid duplicates)
            final existingOutputKeys = walletInstance.outputs
                .map((o) => '${o.txHash}:${o.outputIndex}')
                .toSet();

            final newOutputs = walletResult.outputs.where((o) {
              final key = '${o.txHash}:${o.outputIndex}';
              return !existingOutputKeys.contains(key);
            }).toList();

            if (newOutputs.isNotEmpty) {
              walletInstance.outputs = [...walletInstance.outputs, ...newOutputs];
            }

            // Update heights
            if (response.blockHeight.toInt() > walletInstance.currentHeight) {
              walletInstance.currentHeight = response.blockHeight.toInt();
              _updateBlockHeightFromWallets();
            }
            walletInstance.daemonHeight = response.daemonHeight.toInt();
          }
        }

        // Mark spent outputs
        for (var walletInstance in _openWallets.values) {
          for (int i = 0; i < walletInstance.outputs.length; i++) {
            if (response.spentKeyImages.contains(walletInstance.outputs[i].keyImage)) {
              walletInstance.outputs[i] = OwnedOutput(
                txHash: walletInstance.outputs[i].txHash,
                outputIndex: walletInstance.outputs[i].outputIndex,
                amount: walletInstance.outputs[i].amount,
                amountXmr: walletInstance.outputs[i].amountXmr,
                key: walletInstance.outputs[i].key,
                keyOffset: walletInstance.outputs[i].keyOffset,
                commitmentMask: walletInstance.outputs[i].commitmentMask,
                subaddressIndex: walletInstance.outputs[i].subaddressIndex,
                paymentId: walletInstance.outputs[i].paymentId,
                receivedOutputBytes: walletInstance.outputs[i].receivedOutputBytes,
                blockHeight: walletInstance.outputs[i].blockHeight,
                spent: true,
                keyImage: walletInstance.outputs[i].keyImage,
              );
            }
          }
        }

        // If viewing active wallet, update its display
        if (_activeWalletId != null && _activeWallet != null) {
          _allOutputs = _activeWallet!.outputs;
          // Transactions will be updated on next scan response
        }
      });
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
    return WalletScanService.normalizeNodeUrl(url);
  }

  void _onBlockRefreshTimer() {
    debugPrint('[Dart] Block refresh timer fired');

    if (_isContinuousPaused || !_isContinuousScanning) {
      debugPrint('[Dart] Scan is paused or not scanning, skipping block refresh');
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
      StartContinuousScanRequest(
        nodeUrl: nodeUrl,
        startHeight: Uint64(BigInt.from(_continuousScanCurrentHeight)),
        seed: wallet.seed,
        network: wallet.network,
      ).sendSignalToRust();
    }
  }

  void _onMempoolPollTimer() {
    debugPrint('[Dart] Mempool poll timer fired');
    final seed = _controller.text.trim();
    if (seed.isEmpty) return;

    final nodeUrl = _normalizeNodeUrl(_nodeUrlController.text);

    MempoolScanRequest(
      nodeUrl: nodeUrl,
      seed: seed,
      network: _network,
    ).sendSignalToRust();
  }

  @override
  void dispose() {
    // Cancel stream subscriptions
    _keysDerivedSubscription?.cancel();
    _seedGeneratedSubscription?.cancel();
    _blockScanSubscription?.cancel();
    _daemonHeightSubscription?.cancel();
    _transactionCreatedSubscription?.cancel();
    _transactionBroadcastSubscription?.cancel();
    _syncProgressSubscription?.cancel();
    _spentStatusUpdatedSubscription?.cancel();
    _mempoolScanSubscription?.cancel();

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
    _debounceTimer?.cancel();

    if (_isContinuousScanning) {
      StopScanRequest().sendSignalToRust();
    }
    setState(() {
      _continuousScanCurrentHeight = 0;
      _continuousScanTargetHeight = 0;
      _isSynced = false;
      _allOutputs = [];
      _allTransactions = [];
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

    GenerateSeedRequest().sendSignalToRust();
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

    setState(() {
      _scanError = null;
      _isContinuousPaused = false;
      _isContinuousScanning = true;

      for (var wallet in walletsToScan) {
        wallet.isScanning = true;
      }
    });

    final result = walletsToScan.isEmpty ? KeyParser.parse(_controller.text) : null;
    WalletScanService.startContinuousScan(
      nodeUrl: validation.nodeUrl!,
      startHeight: validation.startHeight!,
      walletsToScan: walletsToScan,
      seed: result?.normalizedInput,
      network: walletsToScan.isEmpty ? _network : null,
    );
  }

  void _pauseContinuousScan() {
    setState(() {
      _isContinuousPaused = true;
      _isContinuousScanning = false;
    });
    WalletScanService.pauseContinuousScan();
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

  int? _parseBlockHeightForContinuous() {
    final startHeightStr = _blockHeightController.text.trim();
    if (startHeightStr.isEmpty) {
      setState(() {
        _scanError = 'Please enter a block height';
      });
      return null;
    }

    final startHeight = int.tryParse(startHeightStr);
    if (startHeight == null || startHeight < 0) {
      setState(() {
        _scanError = 'Invalid block height';
      });
      return null;
    }

    return startHeight;
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

  void _createTransaction() {
    // Build recipient inputs from UI controllers
    final recipientInputs = List.generate(
      _destinationControllers.length,
      (i) => RecipientInput(
        address: _destinationControllers[i].text,
        amount: _amountControllers[i].text,
      ),
    );

    // Validate transaction creation parameters
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

  double _getRecipientsTotal() {
    return OutputUtils.getRecipientsTotal(_amountControllers);
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

  void _showProvePaymentDialog() {
    if (_txResult == null) return;

    final txId = _txResult!.txId;
    final txKey = _txResult!.txKey ?? 'Not available';

    final recipients = <String>[];
    for (int i = 0; i < _destinationControllers.length; i++) {
      final addr = _destinationControllers[i].text.trim();
      if (addr.isNotEmpty) recipients.add(addr);
    }

    // Show loading dialog first
    showDialog(
      context: context,
      barrierDismissible: false,
      builder: (context) => const AlertDialog(
        content: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            CircularProgressIndicator(),
            SizedBox(width: 16),
            Text('Generating OutProof...'),
          ],
        ),
      ),
    );

    // Generate OutProof for the first recipient
    final recipientAddr = recipients.isNotEmpty ? recipients.first : '';
    if (recipientAddr.isEmpty || txKey == 'Not available') {
      Navigator.of(context).pop();
      _showSimpleProofDialog(txId, txKey, recipients);
      return;
    }

    // Subscribe to proof response
    StreamSubscription? sub;
    sub = OutProofGeneratedResponse.rustSignalStream.listen((signal) {
      sub?.cancel();
      Navigator.of(context).pop();

      final response = signal.message;
      if (response.success && response.formatted != null) {
        _showOutProofDialog(response.formatted!, txId, txKey, recipients);
      } else {
        _showSimpleProofDialog(txId, txKey, recipients);
      }
    });

    GenerateOutProofRequest(
      txId: txId,
      txKey: txKey,
      recipientAddress: recipientAddr,
      message: '',
      network: _network,
    ).sendSignalToRust();
  }

  void _showOutProofDialog(String formattedProof, String txId, String txKey, List<String> recipients) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('OutProof'),
        content: SizedBox(
          width: 520,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: Colors.grey.shade100,
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: Colors.grey.shade300),
                  ),
                  child: SelectableText(
                    formattedProof,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 11),
                  ),
                ),
                if (recipients.length > 1) ...[
                  const SizedBox(height: 12),
                  Text(
                    'Note: Proof generated for first recipient only.',
                    style: TextStyle(fontSize: 11, color: Colors.grey.shade600),
                  ),
                ],
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () {
              Clipboard.setData(ClipboardData(text: formattedProof));
              ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('OutProof copied')));
            },
            child: const Text('Copy'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  void _showSimpleProofDialog(String txId, String txKey, List<String> recipients) {
    final allText = [
      'Tx ID: $txId',
      'Tx Key: $txKey',
      ...recipients.map((addr) => 'Address: $addr'),
    ].join('\n');

    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Tx Key'),
        content: SizedBox(
          width: 480,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              CommonWidgets.buildProofRow(label: 'Tx ID', value: txId, context: context),
              const SizedBox(height: 8),
              CommonWidgets.buildProofRow(label: 'Tx Key', value: txKey, context: context),
              if (recipients.isNotEmpty) ...[
                const SizedBox(height: 8),
                ...recipients.map((addr) => CommonWidgets.buildProofRow(label: 'Address', value: addr, context: context)),
              ],
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () {
              Clipboard.setData(ClipboardData(text: allText));
              ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Copied')));
            },
            child: const Text('Copy All'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  Future<void> _copyToClipboard(String text, String label) async {
    await ClipboardUtils.copyToClipboard(context, text, label);
  }

  void _toggleViewMode() {
    _extensionService.isSidePanel
        ? _extensionService.openFullPage()
        : _extensionService.openSidePanel();
  }

  @override
  Widget build(BuildContext context) {
    final isSidePanel = _extensionService.isSidePanel;

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
                      _expandedPanel = (_expandedPanel == index) ? null : index;
                    });
                  },
                  expandIconColor: Theme.of(context).colorScheme.primary,
                  elevation: 1,
                  expandedHeaderPadding: EdgeInsets.zero,
                  children: [
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        final hasData = WalletPersistenceService.hasWalletData(_walletId);
                        final totalBytes = _calculateTotalStorageBytes();

                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 1) ? null : 1;
                            });
                          },
                          child: ListTile(
                            title: const Text(
                              'File Management',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                            subtitle: Text(
                              hasData
                                ? 'Data stored: ${_formatBytes(totalBytes)}'
                                : 'No stored data',
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                        );
                      },
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
                        onWalletChanged: _switchWallet,
                        onLoad: _loadWalletData,
                        onDelete: _clearStoredData,
                        onSave: _saveWalletData,
                        onNew: _startNewWallet,
                        onExport: _exportWallet,
                        onImport: _importWallet,
                        onCloseWallet: _closeWallet,
                      ),
                      isExpanded: _expandedPanel == 1,
                    ),
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 0) ? null : 0;
                            });
                          },
                          child: const ListTile(
                            title: Text(
                              'Seed Phrase',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                          ),
                        );
                      },
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
                      ),
                      isExpanded: _expandedPanel == 0,
                    ),
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 2) ? null : 2;
                            });
                          },
                          child: const ListTile(
                            title: Text(
                              'Keys',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                          ),
                        );
                      },
                      body: KeysDisplayPanel(
                        address: _derivedAddress,
                        secretSpendKey: _secretSpendKey,
                        secretViewKey: _secretViewKey,
                        publicSpendKey: _publicSpendKey,
                        publicViewKey: _publicViewKey,
                        network: _network,
                        onCopyToClipboard: _copyToClipboard,
                      ),
                      isExpanded: _expandedPanel == 2,
                    ),
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 3) ? null : 3;
                            });
                          },
                          child: const ListTile(
                            title: Text(
                              'Scanning',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                          ),
                        );
                      },
                      body: ScanningPanel(
                        nodeUrlController: _nodeUrlController,
                        blockHeightController: _blockHeightController,
                        blockHeightFocusNode: _blockHeightFocusNode,
                        isScanning: _isScanning,
                        isContinuousScanning: _isContinuousScanning,
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
                      isExpanded: _expandedPanel == 3,
                    ),
                    // Transactions Panel
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        final txCount = _allTransactions.length;
                        final incomingCount = _allTransactions.where((t) => t.isIncoming(_allOutputs)).length;
                        final outgoingCount = txCount - incomingCount;
                        final subtitle = txCount == 0
                            ? 'No transactions'
                            : '$txCount transaction${txCount == 1 ? '' : 's'} ($incomingCount in, $outgoingCount out)';

                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 4) ? null : 4;
                            });
                          },
                          child: ListTile(
                            title: const Text(
                              'Transactions',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                            subtitle: Text(
                              subtitle,
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                        );
                      },
                      body: TransactionsPanel(
                        allTransactions: _sortedTransactions(),
                        allOutputs: _allOutputs,
                        currentHeight: _currentHeight,
                        txSortBy: _txSortBy,
                        txSortAscending: _txSortAscending,
                        expandedTransactions: _expandedTransactions,
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
                      isExpanded: _expandedPanel == 4,
                    ),
                    // Coins Panel
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        // Calculate balance from unspent outputs
                        double totalBalance = 0;
                        double unlockedBalance = 0;
                        double selectedBalance = 0;
                        int spendableCount = 0;
                        int lockedCount = 0;
                        int selectedCount = 0;
                        for (var output in _allOutputs) {
                          if (!output.spent) {
                            final amount = double.tryParse(output.amountXmr) ?? 0;
                            totalBalance += amount;
                            final outputHeight = output.blockHeight.toInt();
                            final confirmations = outputHeight > 0 ? _currentHeight - outputHeight : 0;
                            if (confirmations >= 10) {
                              unlockedBalance += amount;
                              spendableCount++;
                              final outputKey = '${output.txHash}:${output.outputIndex}';
                              if (_selectedOutputs.contains(outputKey)) {
                                selectedBalance += amount;
                                selectedCount++;
                              }
                            } else {
                              lockedCount++;
                            }
                          }
                        }
                        final hasLockedBalance = unlockedBalance < totalBalance;
                        final balanceStr = hasLockedBalance
                            ? '${totalBalance.toStringAsFixed(12)} XMR (Unlocked: ${unlockedBalance.toStringAsFixed(12)})'
                            : '${totalBalance.toStringAsFixed(12)} XMR';
                        final outputCountStr = spendableCount > 0
                            ? '$spendableCount spendable output${spendableCount == 1 ? '' : 's'}'
                            : lockedCount > 0
                                ? '$lockedCount locked output${lockedCount == 1 ? '' : 's'}'
                                : 'No outputs';
                        final selectedStr = selectedCount > 0
                            ? ' | Selected: ${selectedBalance.toStringAsFixed(12)} XMR ($selectedCount)'
                            : '';

                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 5) ? null : 5;
                            });
                          },
                          child: ListTile(
                            title: const Text(
                              'Coins',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                            subtitle: Text(
                              '$balanceStr - $outputCountStr$selectedStr',
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                        );
                      },
                      body: OutputsPanel(
                        allOutputs: _allOutputs,
                        currentHeight: _currentHeight,
                        showSpentOutputs: _showSpentOutputs,
                        sortBy: _sortBy,
                        sortAscending: _sortAscending,
                        selectedOutputs: _selectedOutputs,
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
                      isExpanded: _expandedPanel == 5,
                    ),
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 6) ? null : 6;
                            });
                          },
                          child: const ListTile(
                            title: Text(
                              'Create Transaction',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                          ),
                        );
                      },
                      body: CreateTransactionPanel(
                        destinationControllers: _destinationControllers,
                        amountControllers: _amountControllers,
                        isCreatingTx: _isCreatingTx,
                        isBroadcasting: _isBroadcasting,
                        txResult: _txResult,
                        broadcastResult: _broadcastResult,
                        txError: _txError,
                        broadcastError: _broadcastError,
                        onAddRecipient: _addRecipient,
                        onRemoveRecipient: _removeRecipient,
                        onCreateTransaction: _createTransaction,
                        onBroadcastTransaction: _broadcastTransaction,
                        onProvePayment: _showProvePaymentDialog,
                        onAmountChanged: () => setState(() {}),
                      ),
                      isExpanded: _expandedPanel == 6,
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

  // Update transactions from scan results
  void _updateTransactionsFromScan(BlockScanResponse scan) {
    _allTransactions = TransactionUtils.updateTransactionsFromScan(_allTransactions, scan, _allOutputs);
  }

  List<WalletTransaction> _sortedTransactions() {
    return TransactionUtils.sortTransactions(
      _allTransactions,
      _allOutputs,
      _txSortBy,
      _txSortAscending,
      _currentHeight,
    );
  }

  List<OwnedOutput> _sortedOutputs() {
    return TransactionUtils.sortOutputs(
      _allOutputs,
      _sortBy,
      _sortAscending,
      _currentHeight,
      _showSpentOutputs,
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
      final proceed = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          icon: const Icon(Icons.warning, color: Colors.orange, size: 48),
          title: const Text('Security Warning'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Text(
                'You are about to save your wallet data WITHOUT a password.',
                style: TextStyle(fontWeight: FontWeight.bold),
              ),
              const SizedBox(height: 12),
              Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: Colors.red.shade50,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: Colors.red.shade300, width: 2),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Icon(Icons.dangerous, color: Colors.red.shade700, size: 20),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            'YOUR FUNDS ARE AT RISK',
                            style: TextStyle(
                              fontWeight: FontWeight.bold,
                              color: Colors.red.shade900,
                              fontSize: 14,
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    Text(
                      'Anyone with access to your browser can steal your wallet seed and all your Monero.',
                      style: TextStyle(color: Colors.red.shade900, fontSize: 12),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 12),
              const Text(
                'It is STRONGLY RECOMMENDED to use a password.',
                style: TextStyle(fontSize: 13),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              style: TextButton.styleFrom(
                backgroundColor: Colors.green.shade100,
                foregroundColor: Colors.green.shade900,
                padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
              ),
              child: const Text(
                'Cancel (Recommended)',
                style: TextStyle(fontWeight: FontWeight.bold),
              ),
            ),
            ElevatedButton(
              onPressed: () => Navigator.of(context).pop(true),
              style: ElevatedButton.styleFrom(
                backgroundColor: Colors.red,
                foregroundColor: Colors.white,
              ),
              child: const Text('Save Anyway (Unsafe)'),
            ),
          ],
        ),
      );

      if (proceed != true) {
        setState(() {
          _isSaving = false;
        });
        return;
      }
    }

    // Use the persistence service to save wallet data
    final saveResult = await WalletPersistenceService.saveWalletData(
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

    if (success && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Wallet "$walletId" saved successfully'),
          duration: const Duration(seconds: 2),
        ),
      );
    } else if (!success && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Save failed: $_saveError'),
          backgroundColor: Colors.red,
          duration: const Duration(seconds: 3),
        ),
      );
    }
  }

  void _refreshAvailableWallets() {
    final walletIds = WalletPersistenceService.listAvailableWallets();

    setState(() {
      _availableWalletIds = walletIds;
      // If current wallet isn't in the list and there are wallets, select the first one
      if (walletIds.isNotEmpty && !walletIds.contains(_walletId)) {
        _walletId = walletIds.first;
      }
    });
  }

  void _startNewWallet() {
    debugPrint('[WALLET] Starting new wallet (clearing state)');

    // Stop any ongoing scans
    _stopPollingTimers();

    setState(() {
      _walletId = '';
      _controller.text = '';
      _derivedAddress = null;
      _secretSpendKey = null;
      _secretViewKey = null;
      _publicSpendKey = null;
      _publicViewKey = null;
      _allOutputs = [];
      _allTransactions = [];
      _expandedTransactions = {};
      _selectedOutputs = {};
      _isContinuousScanning = false;
      _isContinuousPaused = false;
      _continuousScanCurrentHeight = 0;
      _continuousScanTargetHeight = 0;
      _isSynced = false;
      _daemonHeight = null;
      _scanResult = null;
      _scanError = null;
      _txResult = null;
      _txError = null;
      _broadcastResult = null;
      _broadcastError = null;
      _lastSaveTime = null;
      _loadError = null;
      _saveError = null;
    });

    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Ready for new wallet - generate or enter a seed phrase'),
          duration: Duration(seconds: 2),
        ),
      );
    }
  }

  Future<void> _switchWallet(String newWalletId) async {
    if (newWalletId == _walletId) {
      debugPrint('[WALLET] Already on wallet: $newWalletId');
      return;
    }

    debugPrint('[WALLET] Switching from $_walletId to $newWalletId');

    if (_openWallets.containsKey(newWalletId) && !_openWallets[newWalletId]!.isClosed) {
      _switchToWallet(newWalletId);
      return;
    }

    setState(() {
      _walletId = newWalletId;
    });

    _refreshAvailableWallets();

    if (WalletPersistenceService.hasWalletData(newWalletId)) {
      debugPrint('[WALLET] Wallet $newWalletId has saved data, auto-loading...');
      await _loadWalletData();
    } else {
      setState(() {
        _controller.text = '';
        _derivedAddress = null;
        _secretSpendKey = null;
        _secretViewKey = null;
        _publicSpendKey = null;
        _publicViewKey = null;
        _allOutputs = [];
        _allTransactions = [];
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
      });
    }
  }

  void _openWallet(String walletId, String seed, String network, String address) {
    setState(() {
      final walletInstance = WalletInstance(
        walletId: walletId,
        seed: seed,
        network: network,
        address: address,
        outputs: [],
        currentHeight: 0,
        daemonHeight: 0,
        isScanning: false,
        isClosed: false,
      );

      _openWallets[walletId] = walletInstance;
      _activeWalletId = walletId;

      _allOutputs = walletInstance.outputs;
      _derivedAddress = address;
    });

    debugPrint('[MULTI-WALLET] Opened wallet: $walletId (${_openWallets.length} total open)');

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
    final wallet = _openWallets[walletId];
    if (wallet == null) return;

    final shouldSave = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Close Wallet'),
        content: Text('Save changes to "$walletId" before closing?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Close Without Saving'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(null),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Save & Close'),
          ),
        ],
      ),
    );

    if (shouldSave == null) return;

    if (shouldSave) {
      final previousWalletId = _activeWalletId;
      _switchToWallet(walletId);
      await _saveWalletData();
      if (previousWalletId != null && previousWalletId != walletId) {
        _switchToWallet(previousWalletId);
      }
    }

    setState(() {
      wallet.isClosed = true;
      wallet.isScanning = false;

      if (_activeWalletId == walletId) {
        final remainingWallets = _activeWallets;
        if (remainingWallets.isNotEmpty) {
          _activeWalletId = remainingWallets.first.walletId;
          _switchToWallet(_activeWalletId!);
        } else {
          _activeWalletId = null;
          _allOutputs = [];
        }
      }
    });

    debugPrint('[MULTI-WALLET] Closed wallet: $walletId (${_activeWallets.length} remaining open)');

    _updateBlockHeightFromWallets();

    if (_isContinuousScanning && _activeWallets.isNotEmpty) {
      _pauseContinuousScan();
      Future.delayed(const Duration(milliseconds: 500), () {
        _startContinuousScan();
      });
    }
  }

  void _switchToWallet(String walletId) {
    final wallet = _openWallets[walletId];
    if (wallet == null || wallet.isClosed) return;

    setState(() {
      _activeWalletId = walletId;
      _walletId = walletId;
      _controller.text = wallet.seed;
      _network = wallet.network;
      _derivedAddress = wallet.address;
      _allOutputs = wallet.outputs;
      _daemonHeight = wallet.daemonHeight;
      _continuousScanCurrentHeight = wallet.currentHeight;
    });

    debugPrint('[MULTI-WALLET] Switched to wallet: $walletId');
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
    final loadResult = await WalletPersistenceService.loadWalletData(
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

    // Restore wallet state
    setState(() {
      _controller.text = loadResult.seed!;
      _network = loadResult.network!;
      _derivedAddress = loadResult.address;
      _nodeUrlController.text = loadResult.nodeUrl!;
      _allOutputs = loadResult.outputs!;
      _allTransactions = loadResult.transactions!;
      _continuousScanCurrentHeight = loadResult.continuousScanCurrentHeight!;
      _continuousScanTargetHeight = 0;
      _isSynced = false;
      _daemonHeight = null;
      _isContinuousScanning = false;
      _isContinuousPaused = _continuousScanCurrentHeight > 0;

      // Set block height field to resume scanning from last synced height
      if (_continuousScanCurrentHeight > 0) {
        _blockHeightController.text = _continuousScanCurrentHeight.toString();
        _blockHeightUserEdited = false;
      }

      // Restore selected outputs
      _selectedOutputs = loadResult.selectedOutputs!;

      _isLoadingWallet = false;
      _loadError = null;
    });

    // Open this wallet in multi-wallet mode
    final seed = loadResult.seed!;
    final network = loadResult.network!;
    final address = loadResult.address ?? _derivedAddress ?? '';
    if (seed.isNotEmpty && address.isNotEmpty) {
      _openWallet(_walletId, seed, network, address);
      // Update the opened wallet's outputs
      if (_activeWallet != null) {
        _activeWallet!.outputs = _allOutputs;
        _activeWallet!.currentHeight = _continuousScanCurrentHeight;
        _activeWallet!.daemonHeight = _daemonHeight ?? 0;
      }
    }

    // Derive address to populate keys
    _deriveAddress();

    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Wallet data loaded successfully'),
          duration: Duration(seconds: 2),
        ),
      );
    }
  }

  Future<void> _exportWallet() async {
    // Validation
    if (_walletId.isEmpty) {
      setState(() {
        _exportError = 'No wallet selected for export';
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Please select or save a wallet first'),
            backgroundColor: Colors.orange,
          ),
        );
      }
      return;
    }

    // Check if wallet has saved data
    if (!WalletPersistenceService.hasWalletData(_walletId)) {
      setState(() {
        _exportError = 'No saved data found for this wallet';
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Please save wallet data before exporting'),
            backgroundColor: Colors.orange,
          ),
        );
      }
      return;
    }

    setState(() {
      _isExporting = true;
      _exportError = null;
    });

    // Use the persistence service to export wallet
    final exportResult = await WalletPersistenceService.exportWallet(
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

    if (exportResult.success && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            exportResult.usedSaveAsDialog!
                ? 'Wallet "$_walletId" saved'
                : 'Wallet "$_walletId" exported as ${exportResult.filename}'
          ),
          duration: const Duration(seconds: 3),
        ),
      );
    } else if (!exportResult.success && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Export failed: ${exportResult.error}'),
          backgroundColor: Colors.red,
          duration: const Duration(seconds: 4),
        ),
      );
    }
  }

  Future<void> _importWallet() async {
    setState(() {
      _isImporting = true;
      _importError = null;
    });

    try {
      // Create file input element (WASM-compatible)
      final uploadInput = html.FileUploadInputElement();
      uploadInput.accept = '.monero-wallet,*'; // Prefer .monero-wallet, allow all as fallback
      uploadInput.click();

      // Wait for file selection
      await uploadInput.onChange.first;
      final files = uploadInput.files;
      if (files == null || files.isEmpty) {
        setState(() {
          _isImporting = false;
        });
        return;
      }

      final file = files[0];

      // Extract suggested wallet ID from filename
      final suggestedWalletId = WalletPersistenceService.extractWalletIdFromFilename(file.name);

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

      // Use the persistence service to import wallet
      final importResult = await WalletPersistenceService.importWallet(
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
        // Refresh wallet list
        _refreshAvailableWallets();

        // Switch to imported wallet
        await _switchWallet(walletId);

        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                shouldOverwrite
                    ? 'Wallet "$walletId" overwritten successfully'
                    : 'Wallet "$walletId" imported successfully'
              ),
              duration: const Duration(seconds: 3),
            ),
          );
        }
      } else {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text('Import failed: ${importResult.error}'),
              backgroundColor: Colors.red,
              duration: const Duration(seconds: 4),
            ),
          );
        }
      }
    } catch (e) {
      setState(() {
        _isImporting = false;
        _importError = 'Import failed: $e';
      });

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Import failed: $e'),
            backgroundColor: Colors.red,
            duration: const Duration(seconds: 4),
          ),
        );
      }
    }
  }

  void _clearStoredData() {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Clear Stored Data'),
        content: Text(
          'Are you sure you want to clear stored data for wallet "$_walletId"? This cannot be undone.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          ElevatedButton(
            onPressed: () {
              final deletedWalletId = _walletId;
              WalletPersistenceService.clearWalletData(deletedWalletId);
              Navigator.of(context).pop();
              // Refresh wallet list and clear state
              _refreshAvailableWallets();
              _startNewWallet();
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                  content: Text('Deleted wallet: $deletedWalletId'),
                  duration: const Duration(seconds: 2),
                ),
              );
            },
            style: ElevatedButton.styleFrom(
              backgroundColor: Colors.red,
              foregroundColor: Colors.white,
            ),
            child: const Text('Clear'),
          ),
        ],
      ),
    );
  }
}

