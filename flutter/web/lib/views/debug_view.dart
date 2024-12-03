import 'dart:async';
import 'dart:convert';
import 'dart:html' as html;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:tuple/tuple.dart';
import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';
import '../services/extension_service.dart';
import '../widgets/password_dialog.dart';
import '../services/wallet_storage_service.dart';

class DebugView extends StatefulWidget {
  const DebugView({super.key});

  @override
  State<DebugView> createState() => _DebugViewState();
}

class _DebugViewState extends State<DebugView> {
  final _controller = TextEditingController();
  final _extensionService = ExtensionService();
  final _nodeUrlController = TextEditingController(text: 'http://127.0.0.1:38081');
  final _blockHeightController = TextEditingController();
  final _blockHeightFocusNode = FocusNode();
  bool _blockHeightUserEdited = false;

  // Current wallet ID (multi-wallet support)
  String _walletId = '';
  List<String> _availableWalletIds = [];

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

  /// Returns the current blockchain height for confirmation calculations.
  /// Falls back to 0 if unknown, which safely shows 0 confirmations.
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
  final _storageService = WalletStorageService();
  bool _isSaving = false;
  bool _isLoadingWallet = false;
  String? _saveError;
  String? _loadError;
  String? _lastSaveTime;

  // Helper to get storage key for current wallet
  String get _storageKey => 'monero_wallet_$_walletId';

  // Polling timers (managed in Dart)
  Timer? _blockRefreshTimer;
  Timer? _mempoolPollTimer;
  Timer? _mempoolDelayTimer; // Initial 45s delay before first periodic
  Timer? _countdownTimer;
  int _blockRefreshCountdown = 0;
  int _mempoolCountdown = 0;
  static const _blockRefreshInterval = Duration(seconds: 90);
  static const _mempoolPollInterval = Duration(seconds: 90);
  static const _mempoolPollOffset = Duration(seconds: 45);

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
        _isContinuousScanning = signal.message.isScanning;
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

    // Load available wallets from localStorage
    _refreshAvailableWallets();
  }

  void _startPollingTimers() {
    debugPrint('[Dart] Starting polling timers (block: ${_blockRefreshInterval.inSeconds}s, mempool: ${_mempoolPollInterval.inSeconds}s with ${_mempoolPollOffset.inSeconds}s offset)');
    _stopPollingTimers(); // Cancel any existing timers first

    _blockRefreshCountdown = _blockRefreshInterval.inSeconds;
    _mempoolCountdown = _mempoolPollOffset.inSeconds;

    _countdownTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      setState(() {
        if (_blockRefreshCountdown > 0) _blockRefreshCountdown--;
        if (_mempoolCountdown > 0) _mempoolCountdown--;
      });
    });

    _blockRefreshTimer = Timer.periodic(_blockRefreshInterval, (_) {
      _onBlockRefreshTimer();
      setState(() => _blockRefreshCountdown = _blockRefreshInterval.inSeconds);
    });

    // Start mempool polling with offset to stagger requests
    _mempoolDelayTimer = Timer(_mempoolPollOffset, () {
      _mempoolDelayTimer = null;
      _onMempoolPollTimer(); // First poll at 45s
      setState(() => _mempoolCountdown = _mempoolPollInterval.inSeconds);
      _mempoolPollTimer = Timer.periodic(_mempoolPollInterval, (_) {
        _onMempoolPollTimer();
        setState(() => _mempoolCountdown = _mempoolPollInterval.inSeconds);
      });
    });
  }

  void _stopPollingTimers() {
    if (_blockRefreshTimer != null || _mempoolPollTimer != null || _mempoolDelayTimer != null) {
      debugPrint('[Dart] Stopping polling timers');
    }
    _countdownTimer?.cancel();
    _countdownTimer = null;
    _blockRefreshTimer?.cancel();
    _blockRefreshTimer = null;
    _mempoolDelayTimer?.cancel();
    _mempoolDelayTimer = null;
    _mempoolPollTimer?.cancel();
    _mempoolPollTimer = null;
    _blockRefreshCountdown = 0;
    _mempoolCountdown = 0;
  }

  /// Normalizes a node URL by trimming whitespace and adding http:// if no scheme is present.
  String _normalizeNodeUrl(String url) {
    final trimmed = url.trim();
    if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
      return trimmed;
    }
    return 'http://$trimmed';
  }

  void _onBlockRefreshTimer() {
    debugPrint('[Dart] Block refresh timer fired');
    final seed = _controller.text.trim();
    if (seed.isEmpty) return;

    final nodeUrl = _normalizeNodeUrl(_nodeUrlController.text);

    // Query daemon height - if higher than current, a scan will be triggered
    QueryDaemonHeightRequest(
      nodeUrl: nodeUrl,
    ).sendSignalToRust();

    // Start a continuous scan from current height to check for new blocks
    StartContinuousScanRequest(
      nodeUrl: nodeUrl,
      startHeight: Uint64(BigInt.from(_continuousScanCurrentHeight)),
      seed: seed,
      network: _network,
    ).sendSignalToRust();
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
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _scanError = 'Please enter a seed phrase first';
      });
      return;
    }

    final result = KeyParser.parse(_controller.text);
    if (!result.isValid) {
      setState(() {
        _scanError = 'Invalid seed phrase: ${result.error}';
      });
      return;
    }

    final blockHeightStr = _blockHeightController.text.trim();
    if (blockHeightStr.isEmpty) {
      setState(() {
        _scanError = 'Please enter a block height';
      });
      return;
    }

    final blockHeight = int.tryParse(blockHeightStr);
    if (blockHeight == null || blockHeight < 0) {
      setState(() {
        _scanError = 'Invalid block height';
      });
      return;
    }

    final nodeUrl = _nodeUrlController.text.trim();
    if (nodeUrl.isEmpty) {
      setState(() {
        _scanError = 'Please enter a node URL';
      });
      return;
    }

    setState(() {
      _isScanning = true;
      _scanResult = null;
      _scanError = null;
    });

    // Prepend http:// if not present
    final fullNodeUrl = nodeUrl.startsWith('http://') || nodeUrl.startsWith('https://')
        ? nodeUrl
        : 'http://$nodeUrl';

    ScanBlockRequest(
      nodeUrl: fullNodeUrl,
      blockHeight: Uint64(BigInt.from(blockHeight)),
      seed: result.normalizedInput!,
      network: _network,
    ).sendSignalToRust();
  }

  void _startContinuousScan() {
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _scanError = 'Please enter a seed phrase first';
      });
      return;
    }

    final result = KeyParser.parse(_controller.text);
    if (!result.isValid) {
      setState(() {
        _scanError = 'Invalid seed phrase: ${result.error}';
      });
      return;
    }

    final heightToStart = _parseBlockHeightForContinuous();
    if (heightToStart == null) {
      return;
    }

    final nodeUrl = _nodeUrlController.text.trim();
    if (nodeUrl.isEmpty) {
      setState(() {
        _scanError = 'Please enter a node URL';
      });
      return;
    }

    setState(() {
      _scanError = null;
      _isContinuousPaused = false;
      _isContinuousScanning = true;
    });

    // Prepend http:// if not present
    final fullNodeUrl = nodeUrl.startsWith('http://') || nodeUrl.startsWith('https://')
        ? nodeUrl
        : 'http://$nodeUrl';

    StartContinuousScanRequest(
      nodeUrl: fullNodeUrl,
      startHeight: Uint64(BigInt.from(heightToStart)),
      seed: result.normalizedInput!,
      network: _network,
    ).sendSignalToRust();
  }

  void _pauseContinuousScan() {
    setState(() {
      _isContinuousPaused = true;
      _isContinuousScanning = false;
    });
    StopScanRequest().sendSignalToRust();
  }

  void _scanMempool() {
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _scanError = 'Please enter a seed phrase first';
      });
      return;
    }

    final result = KeyParser.parse(_controller.text);
    if (!result.isValid) {
      setState(() {
        _scanError = 'Invalid seed phrase: ${result.error}';
      });
      return;
    }

    final nodeUrl = _nodeUrlController.text.trim();
    if (nodeUrl.isEmpty) {
      setState(() {
        _scanError = 'Please enter a node URL';
      });
      return;
    }

    setState(() {
      _isScanningMempool = true;
      _scanError = null;
    });

    // Prepend http:// if not present
    final fullNodeUrl = nodeUrl.startsWith('http://') || nodeUrl.startsWith('https://')
        ? nodeUrl
        : 'http://$nodeUrl';

    MempoolScanRequest(
      nodeUrl: fullNodeUrl,
      seed: result.normalizedInput!,
      network: _network,
    ).sendSignalToRust();
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
    final result = KeyParser.parse(_controller.text);

    if (!result.isValid || result.normalizedInput == null) {
      setState(() {
        _txError = 'Please enter a valid seed phrase first';
      });
      return;
    }

    if (_allOutputs.isEmpty) {
      setState(() {
        _txError = 'No outputs available. Scan blocks to find outputs first.';
      });
      return;
    }

    // Validate all recipients
    List<Recipient> recipients = [];
    int totalAtomic = 0;

    for (int i = 0; i < _destinationControllers.length; i++) {
      final destination = _destinationControllers[i].text.trim();
      if (destination.isEmpty) {
        setState(() {
          _txError = 'Please enter a destination address for recipient ${i + 1}';
        });
        return;
      }

      final amountStr = _amountControllers[i].text.trim();
      if (amountStr.isEmpty) {
        setState(() {
          _txError = 'Please enter an amount for recipient ${i + 1}';
        });
        return;
      }

      final amountXmr = double.tryParse(amountStr);
      if (amountXmr == null || amountXmr <= 0) {
        setState(() {
          _txError = 'Please enter a valid amount for recipient ${i + 1}';
        });
        return;
      }

      // Convert XMR to atomic units (1 XMR = 1e12 atomic units)
      final amountAtomic = (amountXmr * 1e12).round();
      if (amountAtomic <= 0) {
        setState(() {
          _txError = 'Amount too small for recipient ${i + 1}';
        });
        return;
      }

      totalAtomic += amountAtomic;
      recipients.add(Recipient(
        address: destination,
        amount: Uint64(BigInt.from(amountAtomic)),
      ));
    }

    // Validate coin selection if outputs are selected
    if (_selectedOutputs.isNotEmpty) {
      final selectedTotal = _getSelectedOutputsTotal();
      if (selectedTotal < totalAtomic) {
        final selectedXmr = (selectedTotal / 1e12).toStringAsFixed(12);
        final totalXmr = (totalAtomic / 1e12).toStringAsFixed(12);
        setState(() {
          _txError = 'Selected outputs ($selectedXmr XMR) insufficient for $totalXmr XMR + fees';
        });
        return;
      }
    }

    final nodeUrl = _nodeUrlController.text.trim();
    if (nodeUrl.isEmpty) {
      setState(() {
        _txError = 'Please enter a node URL';
      });
      return;
    }

    setState(() {
      _isCreatingTx = true;
      _txResult = null;
      _txError = null;
    });

    final fullNodeUrl = nodeUrl.startsWith('http://') || nodeUrl.startsWith('https://')
        ? nodeUrl
        : 'http://$nodeUrl';

    CreateTransactionRequest(
      nodeUrl: fullNodeUrl,
      seed: result.normalizedInput!,
      network: _network,
      recipients: recipients,
      selectedOutputs: _selectedOutputs.isNotEmpty ? _selectedOutputs.toList() : null,
    ).sendSignalToRust();
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
    double total = 0;
    for (var controller in _amountControllers) {
      final amount = double.tryParse(controller.text.trim());
      if (amount != null && amount > 0) {
        total += amount;
      }
    }
    return total;
  }

  int _getSelectedOutputsTotal() {
    int total = 0;
    for (var output in _allOutputs) {
      if (output.spent) continue;
      final outputHeight = output.blockHeight.toInt();
      final confirmations = outputHeight > 0 ? _currentHeight - outputHeight : 0;
      if (confirmations < 10) continue;
      final outputKey = '${output.txHash}:${output.outputIndex}';
      if (_selectedOutputs.contains(outputKey)) {
        total += output.amount.toInt();
      }
    }
    return total;
  }

  void _broadcastTransaction() {
    if (_txResult == null || _txResult!.txBlob == null) {
      setState(() {
        _broadcastError = 'No transaction to broadcast';
      });
      return;
    }

    final nodeUrl = _nodeUrlController.text.trim();
    if (nodeUrl.isEmpty) {
      setState(() {
        _broadcastError = 'Please enter a node URL';
      });
      return;
    }

    setState(() {
      _isBroadcasting = true;
      _broadcastResult = null;
      _broadcastError = null;
    });

    final fullNodeUrl = nodeUrl.startsWith('http://') || nodeUrl.startsWith('https://')
        ? nodeUrl
        : 'http://$nodeUrl';

    BroadcastTransactionRequest(
      nodeUrl: fullNodeUrl,
      txBlob: _txResult!.txBlob!,
      spentOutputHashes: _txResult!.spentOutputHashes,
    ).sendSignalToRust();
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
              _buildProofRow('Tx ID', txId, context),
              const SizedBox(height: 8),
              _buildProofRow('Tx Key', txKey, context),
              if (recipients.isNotEmpty) ...[
                const SizedBox(height: 8),
                ...recipients.map((addr) => _buildProofRow('Address', addr, context)),
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

  Widget _buildProofRow(String label, String value, BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 70,
          child: Text(label, style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 12)),
        ),
        Expanded(
          child: SelectableText(value, style: const TextStyle(fontFamily: 'monospace', fontSize: 11)),
        ),
        IconButton(
          icon: const Icon(Icons.copy, size: 16),
          padding: EdgeInsets.zero,
          constraints: const BoxConstraints(),
          tooltip: 'Copy',
          onPressed: () {
            Clipboard.setData(ClipboardData(text: value));
            ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$label copied')));
          },
        ),
      ],
    );
  }

  Future<void> _copyToClipboard(String text, String label) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('$label copied to clipboard'),
          duration: const Duration(seconds: 2),
        ),
      );
    }
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
                        final hasData = html.window.localStorage.containsKey(_storageKey);
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
                              hasData ? 'Wallet data stored' : 'No stored data',
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                        );
                      },
                      body: Padding(
                        padding: const EdgeInsets.all(16.0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            // Wallet Switcher - only show if there are wallets
                            if (_availableWalletIds.isNotEmpty) ...[
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.grey.shade100,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.grey.shade300),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Row(
                                      children: [
                                        Icon(Icons.account_balance_wallet, color: Colors.grey.shade700, size: 20),
                                        const SizedBox(width: 8),
                                        Text(
                                          'Stored Wallets',
                                          style: TextStyle(
                                            fontWeight: FontWeight.bold,
                                            color: Colors.grey.shade900,
                                          ),
                                        ),
                                      ],
                                    ),
                                    const SizedBox(height: 12),
                                    DropdownButtonFormField<String>(
                                      value: _availableWalletIds.contains(_walletId) ? _walletId : null,
                                      decoration: const InputDecoration(
                                        labelText: 'Select Wallet',
                                        border: OutlineInputBorder(),
                                        contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                                      ),
                                      items: _availableWalletIds.map((id) {
                                        return DropdownMenuItem(
                                          value: id,
                                          child: Text(id),
                                        );
                                      }).toList(),
                                      onChanged: (newId) {
                                        if (newId != null) {
                                          _switchWallet(newId);
                                        }
                                      },
                                    ),
                                    if (_lastSaveTime != null) ...[
                                      const SizedBox(height: 8),
                                      Text(
                                        'Last saved: $_lastSaveTime',
                                        style: TextStyle(
                                          fontSize: 12,
                                          color: Colors.grey.shade700,
                                        ),
                                      ),
                                    ],
                                    const SizedBox(height: 12),
                                    Row(
                                      children: [
                                        Expanded(
                                          child: ElevatedButton.icon(
                                            onPressed: _isLoadingWallet ? null : _loadWalletData,
                                            icon: _isLoadingWallet
                                                ? const SizedBox(
                                                    width: 16,
                                                    height: 16,
                                                    child: CircularProgressIndicator(strokeWidth: 2),
                                                  )
                                                : const Icon(Icons.folder_open),
                                            label: Text(_isLoadingWallet ? 'Loading...' : 'Load'),
                                            style: ElevatedButton.styleFrom(
                                              backgroundColor: Colors.green,
                                              foregroundColor: Colors.white,
                                            ),
                                          ),
                                        ),
                                        const SizedBox(width: 8),
                                        Expanded(
                                          child: OutlinedButton.icon(
                                            onPressed: _clearStoredData,
                                            icon: const Icon(Icons.delete_outline),
                                            label: const Text('Delete'),
                                            style: OutlinedButton.styleFrom(
                                              foregroundColor: Colors.red,
                                            ),
                                          ),
                                        ),
                                      ],
                                    ),
                                  ],
                                ),
                              ),
                              const SizedBox(height: 12),
                            ],
                            Row(
                              children: [
                                Expanded(
                                  child: ElevatedButton.icon(
                                    onPressed: _isSaving ? null : _saveWalletData,
                                    icon: _isSaving
                                        ? const SizedBox(
                                            width: 16,
                                            height: 16,
                                            child: CircularProgressIndicator(strokeWidth: 2),
                                          )
                                        : const Icon(Icons.save),
                                    label: Text(_isSaving ? 'Saving...' : 'Save Wallet Data'),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                OutlinedButton.icon(
                                  onPressed: _startNewWallet,
                                  icon: const Icon(Icons.add),
                                  label: const Text('New'),
                                ),
                              ],
                            ),
                            if (_saveError != null) ...[
                              const SizedBox(height: 12),
                              Container(
                                padding: const EdgeInsets.all(8),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(4),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: Text(
                                  _saveError!,
                                  style: TextStyle(color: Colors.red.shade900, fontSize: 12),
                                ),
                              ),
                            ],
                            if (_loadError != null) ...[
                              const SizedBox(height: 12),
                              Container(
                                padding: const EdgeInsets.all(8),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(4),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: Text(
                                  _loadError!,
                                  style: TextStyle(color: Colors.red.shade900, fontSize: 12),
                                ),
                              ),
                            ],
                          ],
                        ),
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
                      body: Padding(
                        padding: const EdgeInsets.all(16.0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Row(
                              children: [
                                ElevatedButton.icon(
                                  onPressed: _generateSeed,
                                  icon: const Icon(Icons.auto_awesome),
                                  label: const Text('Generate'),
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: DropdownButtonFormField<String>(
                                    value: _seedType,
                                    decoration: const InputDecoration(
                                      labelText: 'Seed Type',
                                      border: OutlineInputBorder(),
                                      contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                                    ),
                                    items: const [
                                      DropdownMenuItem(value: '25 word', child: Text('25 word')),
                                    ],
                                    onChanged: null,
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: DropdownButtonFormField<String>(
                                    value: _network,
                                    decoration: const InputDecoration(
                                      labelText: 'Network',
                                      border: OutlineInputBorder(),
                                      contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                                    ),
                                    items: const [
                                      DropdownMenuItem(value: 'mainnet', child: Text('Mainnet')),
                                      DropdownMenuItem(value: 'testnet', child: Text('Testnet')),
                                      DropdownMenuItem(value: 'stagenet', child: Text('Stagenet')),
                                    ],
                                    onChanged: (value) {
                                      if (value != null) {
                                        setState(() {
                                          _network = value;
                                        });
                                        _deriveAddress();
                                      }
                                    },
                                  ),
                                ),
                              ],
                            ),
                            const SizedBox(height: 16),
                            Row(
                              children: [
                                Expanded(
                                  child: TextField(
                                    controller: _controller,
                                    decoration: InputDecoration(
                                      labelText: 'Seed Phrase',
                                      hintText: 'Enter or generate a 25-word seed phrase',
                                      border: const OutlineInputBorder(),
                                      errorText: _validationError,
                                    ),
                                    maxLines: 3,
                                  ),
                                ),
                                IconButton(
                                  icon: const Icon(Icons.copy_outlined),
                                  onPressed: () => _copyToClipboard(_controller.text, 'Seed'),
                                  tooltip: 'Copy seed',
                                ),
                              ],
                            ),
                            if (_responseError != null) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: SelectableText(
                                  'Error: $_responseError',
                                  style: TextStyle(color: Colors.red.shade900),
                                ),
                              ),
                            ],
                          ],
                        ),
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
                      body: Builder(
                        builder: (context) {
                          final address = _derivedAddress;
                          if (address == null) {
                            return const Padding(
                              padding: EdgeInsets.all(16.0),
                              child: Text('Enter a seed phrase to view keys'),
                            );
                          }
                          return Column(
                              children: [
                                Padding(
                                  padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                                  child: Column(
                                    crossAxisAlignment: CrossAxisAlignment.start,
                                    children: [
                                      Text(
                                        '${_network[0].toUpperCase()}${_network.substring(1)} Address',
                                        style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                                      ),
                                      const SizedBox(height: 4),
                                      Row(
                                        children: [
                                          Expanded(
                                            child: SelectableText(
                                              address,
                                              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                                            ),
                                          ),
                                          IconButton(
                                            icon: const Icon(Icons.copy_outlined, size: 16),
                                            onPressed: () => _copyToClipboard(address, 'Address'),
                                            tooltip: 'Copy address',
                                            padding: EdgeInsets.zero,
                                            constraints: const BoxConstraints(),
                                          ),
                                        ],
                                      ),
                                    ],
                                  ),
                                ),
                                const Divider(height: 1),
                                _buildKeyRow('Secret Spend Key', _secretSpendKey ?? 'TODO'),
                                _buildKeyRow('Secret View Key', _secretViewKey ?? 'TODO'),
                                _buildKeyRow('Public Spend Key', _publicSpendKey ?? 'TODO'),
                                _buildKeyRow('Public View Key', _publicViewKey ?? 'TODO'),
                              ],
                            );
                        },
                      ),
                      isExpanded: _expandedPanel == 1,
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
                              'Scanning',
                              style: TextStyle(fontWeight: FontWeight.bold),
                            ),
                          ),
                        );
                      },
                      body: Padding(
                        padding: const EdgeInsets.all(16.0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            TextField(
                              controller: _nodeUrlController,
                              decoration: const InputDecoration(
                                labelText: 'Node Address',
                                hintText: '127.0.0.1:38081',
                                border: OutlineInputBorder(),
                                helperText: 'For local stagenet node',
                              ),
                            ),
                            const SizedBox(height: 16),
                            Row(
                              children: [
                                Expanded(
                                  child: TextField(
                                    controller: _blockHeightController,
                                    focusNode: _blockHeightFocusNode,
                                    decoration: const InputDecoration(
                                      labelText: 'Block Height',
                                      hintText: 'Block height for scan',
                                      border: OutlineInputBorder(),
                                    ),
                                    keyboardType: TextInputType.number,
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: ElevatedButton.icon(
                                    onPressed: (_isScanning || _isContinuousScanning) ? null : _scanBlock,
                                    icon: _isScanning
                                        ? const SizedBox(
                                            width: 16,
                                            height: 16,
                                            child: CircularProgressIndicator(strokeWidth: 2),
                                          )
                                        : const Icon(Icons.search),
                                    label: Text(_isScanning ? 'Scanning...' : 'Scan One'),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: ElevatedButton.icon(
                                    onPressed: _isScanning
                                        ? null
                                        : _isContinuousScanning
                                            ? _pauseContinuousScan
                                            : _startContinuousScan,
                                    icon: Icon(_isContinuousScanning ? Icons.pause : Icons.play_arrow),
                                    label: Text(_continuousScanButtonLabel()),
                                    style: ElevatedButton.styleFrom(
                                      backgroundColor: _continuousScanButtonColor(),
                                      foregroundColor: Colors.white,
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: ElevatedButton.icon(
                                    onPressed: (_isScanningMempool || _controller.text.trim().isEmpty)
                                        ? null
                                        : _scanMempool,
                                    icon: _isScanningMempool
                                        ? const SizedBox(
                                            width: 16,
                                            height: 16,
                                            child: CircularProgressIndicator(strokeWidth: 2),
                                          )
                                        : const Icon(Icons.memory),
                                    label: Text(_isScanningMempool ? 'Scanning...' : 'Scan Mempool'),
                                    style: ElevatedButton.styleFrom(
                                      backgroundColor: Colors.purple,
                                      foregroundColor: Colors.white,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                            if (_isContinuousScanning || _isSynced) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: _isSynced ? Colors.green.shade50 : Colors.blue.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(
                                    color: _isSynced ? Colors.green.shade200 : Colors.blue.shade200,
                                  ),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Row(
                                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                                      children: [
                                        Text(
                                          _isSynced ? 'Synced' : 'Scanning Progress',
                                          style: TextStyle(
                                            fontWeight: FontWeight.bold,
                                            color: _isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                                          ),
                                        ),
                                        if (_isSynced)
                                          Container(
                                            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                                            decoration: BoxDecoration(
                                              color: Colors.green,
                                              borderRadius: BorderRadius.circular(12),
                                            ),
                                            child: const Text(
                                              'SYNCED',
                                              style: TextStyle(
                                                color: Colors.white,
                                                fontSize: 10,
                                                fontWeight: FontWeight.bold,
                                              ),
                                            ),
                                          ),
                                      ],
                                    ),
                                    const SizedBox(height: 8),
                                    Text(
                                      'Block $_continuousScanCurrentHeight / $_continuousScanTargetHeight',
                                      style: TextStyle(
                                        fontSize: 12,
                                        color: _isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                                      ),
                                    ),
                                    const SizedBox(height: 8),
                                    LinearProgressIndicator(
                                      value: _continuousScanTargetHeight > 0
                                          ? _continuousScanCurrentHeight / _continuousScanTargetHeight
                                          : 0,
                                      backgroundColor: Colors.grey.shade300,
                                      valueColor: AlwaysStoppedAnimation<Color>(
                                        _isSynced ? Colors.green : Colors.blue,
                                      ),
                                    ),
                                    const SizedBox(height: 4),
                                    Text(
                                      _continuousScanTargetHeight > 0
                                          ? '${((_continuousScanCurrentHeight / _continuousScanTargetHeight) * 100).toStringAsFixed(1)}%'
                                          : '0%',
                                      style: TextStyle(
                                        fontSize: 12,
                                        fontWeight: FontWeight.bold,
                                        color: _isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                                      ),
                                    ),
                                    // Polling countdown when synced
                                    if (_isSynced && (_blockRefreshCountdown > 0 || _mempoolCountdown > 0)) ...[
                                      const SizedBox(height: 12),
                                      Text(
                                        'Next poll: ${_mempoolCountdown > 0 && (_blockRefreshCountdown == 0 || _mempoolCountdown < _blockRefreshCountdown) ? _mempoolCountdown : _blockRefreshCountdown}s',
                                        style: TextStyle(
                                          fontSize: 11,
                                          color: Colors.green.shade700,
                                        ),
                                      ),
                                    ],
                                  ],
                                ),
                              ),
                            ],
                            const SizedBox(height: 16),
                            if (_scanError != null) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: SelectableText(
                                  'Scan Error: $_scanError',
                                  style: TextStyle(color: Colors.red.shade900),
                                ),
                              ),
                            ],
                            if (_scanResult != null) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.green.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.green.shade200),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      'Scan Results',
                                      style: TextStyle(
                                        fontWeight: FontWeight.bold,
                                        color: Colors.green.shade900,
                                        fontSize: 16,
                                      ),
                                    ),
                                    const SizedBox(height: 8),
                                    _buildScanResultRow('Block Height', _scanResult!.blockHeight.toString()),
                                    _buildScanResultRow('Block Hash', _scanResult!.blockHash),
                                    _buildScanResultRow('Timestamp', DateTime.fromMillisecondsSinceEpoch(
                                      _scanResult!.blockTimestamp.toInt() * 1000,
                                    ).toString()),
                                    _buildScanResultRow('Transactions', _scanResult!.txCount.toString()),
                                    _buildScanResultRow('Outputs Found', _scanResult!.outputs.length.toString()),
                                    if (_scanResult!.outputs.isNotEmpty) ...[
                                      const Divider(height: 24),
                                      Text(
                                        'Owned Outputs:',
                                        style: TextStyle(
                                          fontWeight: FontWeight.bold,
                                          color: Colors.green.shade900,
                                        ),
                                      ),
                                      const SizedBox(height: 8),
                                      ..._scanResult!.outputs.map((output) => Card(
                                        margin: const EdgeInsets.only(bottom: 8),
                                        child: Padding(
                                          padding: const EdgeInsets.all(12),
                                          child: Column(
                                            crossAxisAlignment: CrossAxisAlignment.start,
                                            children: [
                                              Row(
                                                children: [
                                                  Text(
                                                    'Amount: ${output.amountXmr} XMR',
                                                    style: const TextStyle(
                                                      fontWeight: FontWeight.bold,
                                                      fontSize: 14,
                                                    ),
                                                  ),
                                                ],
                                              ),
                                              const SizedBox(height: 4),
                                              Text('TX Hash: ${output.txHash}', style: const TextStyle(fontSize: 10, fontFamily: 'monospace')),
                                              Text('Output Index: ${output.outputIndex}', style: const TextStyle(fontSize: 10)),
                                              if (output.subaddressIndex != null)
                                                Text('Subaddress: ${output.subaddressIndex!.item1}/${output.subaddressIndex!.item2}', style: const TextStyle(fontSize: 10)),
                                              if (output.paymentId != null)
                                                Text('Payment ID: ${output.paymentId}', style: const TextStyle(fontSize: 10)),
                                            ],
                                          ),
                                        ),
                                      )),
                                    ],
                                  ],
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                      isExpanded: _expandedPanel == 2,
                    ),
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
                              _expandedPanel = (_expandedPanel == 3) ? null : 3;
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
                      body: Padding(
                        padding: const EdgeInsets.all(16.0),
                        child: _allOutputs.isEmpty
                            ? const Center(
                                child: Padding(
                                  padding: EdgeInsets.all(16.0),
                                  child: Text(
                                    'No outputs found. Scan blocks to find outputs.',
                                    style: TextStyle(color: Colors.grey),
                                  ),
                                ),
                              )
                            : Column(
                                crossAxisAlignment: CrossAxisAlignment.stretch,
                                children: [
                                  Padding(
                                    padding: const EdgeInsets.only(bottom: 12),
                                    child: Row(
                                      children: [
                                        if (_allOutputs.any((o) => o.spent)) ...[
                                          Checkbox(
                                            value: _showSpentOutputs,
                                            onChanged: (value) {
                                              setState(() {
                                                _showSpentOutputs = value ?? false;
                                              });
                                            },
                                          ),
                                          GestureDetector(
                                            onTap: () {
                                              setState(() {
                                                _showSpentOutputs = !_showSpentOutputs;
                                              });
                                            },
                                            child: const Text('Show spent'),
                                          ),
                                          const SizedBox(width: 12),
                                        ],
                                        const Text('Select: ', style: TextStyle(fontSize: 12)),
                                        _buildSelectButton('All', _selectAllSpendable),
                                        const SizedBox(width: 4),
                                        _buildSelectButton('None', _clearSelection),
                                        const Spacer(),
                                        const Text('Sort: ', style: TextStyle(fontSize: 12)),
                                        _buildSortButton('Confirms', 'confirms'),
                                        const SizedBox(width: 4),
                                        _buildSortButton('Value', 'value'),
                                      ],
                                    ),
                                  ),
                                  ..._sortedOutputs().map((output) {
                                  final outputHeight = output.blockHeight.toInt();
                                  final confirmations = outputHeight > 0
                                      ? _currentHeight - outputHeight
                                      : 0;
                                  final isSpendable = confirmations >= 10 && !output.spent;
                                  final statusColor = output.spent
                                      ? Colors.grey
                                      : isSpendable
                                          ? Colors.green
                                          : Colors.orange;
                                  final statusText = output.spent
                                      ? 'SPENT'
                                      : isSpendable
                                          ? 'SPENDABLE'
                                          : 'LOCKED ($confirmations/10)';

                                  final outputKey = '${output.txHash}:${output.outputIndex}';
                                  final isSelected = _selectedOutputs.contains(outputKey);

                                  return Card(
                                    margin: const EdgeInsets.only(bottom: 12),
                                    elevation: 2,
                                    child: Padding(
                                      padding: const EdgeInsets.all(12),
                                      child: Column(
                                        crossAxisAlignment: CrossAxisAlignment.start,
                                        children: [
                                          Row(
                                            mainAxisAlignment: MainAxisAlignment.spaceBetween,
                                            children: [
                                              Row(
                                                children: [
                                                  if (isSpendable)
                                                    SizedBox(
                                                      width: 24,
                                                      height: 24,
                                                      child: Checkbox(
                                                        value: isSelected,
                                                        onChanged: (value) {
                                                          setState(() {
                                                            if (value == true) {
                                                              _selectedOutputs.add(outputKey);
                                                            } else {
                                                              _selectedOutputs.remove(outputKey);
                                                            }
                                                          });
                                                        },
                                                      ),
                                                    ),
                                                  if (isSpendable) const SizedBox(width: 8),
                                                  Text(
                                                    '${output.amountXmr} XMR',
                                                    style: TextStyle(
                                                      fontWeight: FontWeight.bold,
                                                      fontSize: 16,
                                                      color: output.spent ? Colors.grey.shade600 : Colors.black,
                                                    ),
                                                  ),
                                                ],
                                              ),
                                              Container(
                                                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                                                decoration: BoxDecoration(
                                                  color: statusColor.withOpacity(0.1),
                                                  borderRadius: BorderRadius.circular(4),
                                                  border: Border.all(color: statusColor),
                                                ),
                                                child: Text(
                                                  statusText,
                                                  style: TextStyle(
                                                    color: statusColor.shade800,
                                                    fontSize: 10,
                                                    fontWeight: FontWeight.bold,
                                                  ),
                                                ),
                                              ),
                                            ],
                                          ),
                                          const SizedBox(height: 8),
                                          _buildOutputDetailRow('TX Hash', output.txHash, mono: true),
                                          _buildOutputDetailRow('Output Index', '${output.outputIndex}'),
                                          _buildOutputDetailRow('Block Height', '$outputHeight'),
                                          if (output.subaddressIndex != null)
                                            _buildOutputDetailRow(
                                              'Subaddress',
                                              '${output.subaddressIndex!.item1}/${output.subaddressIndex!.item2}',
                                            ),
                                          if (output.paymentId != null)
                                            _buildOutputDetailRow('Payment ID', output.paymentId!, mono: true),
                                        ],
                                      ),
                                    ),
                                  );
                                }),
                                ],
                              ),
                      ),
                      isExpanded: _expandedPanel == 3,
                    ),
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return GestureDetector(
                          onTap: () {
                            setState(() {
                              _expandedPanel = (_expandedPanel == 4) ? null : 4;
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
                      body: Padding(
                        padding: const EdgeInsets.all(16.0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Row(
                              mainAxisAlignment: MainAxisAlignment.spaceBetween,
                              children: [
                                Text(
                                  'Recipients: ${_destinationControllers.length}/15',
                                  style: const TextStyle(fontWeight: FontWeight.bold),
                                ),
                                Text(
                                  'Total: ${_getRecipientsTotal().toStringAsFixed(12)} XMR',
                                  style: const TextStyle(fontWeight: FontWeight.bold, color: Colors.blue),
                                ),
                              ],
                            ),
                            const SizedBox(height: 12),
                            ...List.generate(_destinationControllers.length, (index) {
                              return Container(
                                margin: const EdgeInsets.only(bottom: 12),
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  border: Border.all(color: Colors.grey.shade300),
                                  borderRadius: BorderRadius.circular(8),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.stretch,
                                  children: [
                                    Row(
                                      children: [
                                        Text(
                                          'Recipient ${index + 1}',
                                          style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 12),
                                        ),
                                        const Spacer(),
                                        if (_destinationControllers.length > 1)
                                          IconButton(
                                            icon: const Icon(Icons.close, size: 18),
                                            onPressed: () => _removeRecipient(index),
                                            tooltip: 'Remove recipient',
                                            padding: EdgeInsets.zero,
                                            constraints: const BoxConstraints(),
                                          ),
                                      ],
                                    ),
                                    const SizedBox(height: 8),
                                    TextField(
                                      controller: _destinationControllers[index],
                                      decoration: const InputDecoration(
                                        labelText: 'Address',
                                        hintText: 'Enter recipient Monero address',
                                        border: OutlineInputBorder(),
                                        contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                                      ),
                                      style: const TextStyle(fontSize: 12),
                                    ),
                                    const SizedBox(height: 8),
                                    TextField(
                                      controller: _amountControllers[index],
                                      decoration: const InputDecoration(
                                        labelText: 'Amount (XMR)',
                                        border: OutlineInputBorder(),
                                        contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                                      ),
                                      keyboardType: const TextInputType.numberWithOptions(decimal: true),
                                      style: const TextStyle(fontSize: 12),
                                      onChanged: (_) => setState(() {}),
                                    ),
                                  ],
                                ),
                              );
                            }),
                            if (_destinationControllers.length < 15)
                              OutlinedButton.icon(
                                onPressed: _addRecipient,
                                icon: const Icon(Icons.add),
                                label: const Text('Add Recipient'),
                              ),
                            const SizedBox(height: 16),
                            ElevatedButton.icon(
                              onPressed: _isCreatingTx ? null : _createTransaction,
                              icon: _isCreatingTx
                                  ? const SizedBox(
                                      width: 16,
                                      height: 16,
                                      child: CircularProgressIndicator(strokeWidth: 2),
                                    )
                                  : const Icon(Icons.send),
                              label: Text(_isCreatingTx ? 'Creating Transaction...' : 'Create Transaction'),
                            ),
                            if (_txError != null) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: SelectableText(
                                  'Transaction Error: $_txError',
                                  style: TextStyle(color: Colors.red.shade900),
                                ),
                              ),
                            ],
                            if (_txResult != null && _txResult!.success) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.green.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.green.shade200),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      'Transaction Created',
                                      style: TextStyle(
                                        fontWeight: FontWeight.bold,
                                        color: Colors.green.shade900,
                                        fontSize: 16,
                                      ),
                                    ),
                                    const SizedBox(height: 8),
                                    _buildScanResultRow('TX ID', _txResult!.txId),
                                    _buildScanResultRow('Fee', '${(_txResult!.fee.toInt() / 1e12).toStringAsFixed(12)} XMR'),
                                    if (_txResult!.txBlob != null)
                                      _buildScanResultRow('TX Blob', '${_txResult!.txBlob!.substring(0, 64)}...'),
                                    const SizedBox(height: 12),
                                    ElevatedButton.icon(
                                      onPressed: _isBroadcasting ? null : _broadcastTransaction,
                                      icon: _isBroadcasting
                                          ? const SizedBox(
                                              width: 16,
                                              height: 16,
                                              child: CircularProgressIndicator(strokeWidth: 2),
                                            )
                                          : const Icon(Icons.upload),
                                      label: Text(_isBroadcasting ? 'Broadcasting...' : 'Broadcast Transaction'),
                                      style: ElevatedButton.styleFrom(
                                        backgroundColor: Colors.green.shade700,
                                        foregroundColor: Colors.white,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                            if (_broadcastError != null) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: SelectableText(
                                  'Broadcast Error: $_broadcastError',
                                  style: TextStyle(color: Colors.red.shade900),
                                ),
                              ),
                            ],
                            if (_broadcastResult != null && _broadcastResult!.success) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.blue.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.blue.shade200),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      'Transaction Broadcast Successfully!',
                                      style: TextStyle(
                                        fontWeight: FontWeight.bold,
                                        color: Colors.blue.shade900,
                                        fontSize: 16,
                                      ),
                                    ),
                                    const SizedBox(height: 8),
                                    Text(
                                      'The transaction has been submitted to the network.',
                                      style: TextStyle(color: Colors.blue.shade900),
                                    ),
                                    if (_txResult?.txKey != null) ...[
                                      const SizedBox(height: 12),
                                      ElevatedButton(
                                        onPressed: _showProvePaymentDialog,
                                        style: ElevatedButton.styleFrom(
                                          backgroundColor: Colors.blue.shade700,
                                          foregroundColor: Colors.white,
                                        ),
                                        child: const Text('Prove Payment'),
                                      ),
                                    ],
                                  ],
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                      isExpanded: _expandedPanel == 4,
                    ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildKeyRow(String label, String value) {
    final bool isTodo = value == 'TODO';
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                ),
                const SizedBox(height: 4),
                SelectableText(
                  value,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                ),
              ],
            ),
          ),
          IconButton(
            icon: const Icon(Icons.copy_outlined, size: 16),
            onPressed: !isTodo ? () => _copyToClipboard(value, label) : null,
            tooltip: isTodo ? null : 'Copy $label',
            padding: EdgeInsets.zero,
            constraints: const BoxConstraints(),
          ),
        ],
      ),
    );
  }

  Widget _buildScanResultRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 120,
            child: Text(
              '$label:',
              style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 12),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: const TextStyle(fontSize: 12, fontFamily: 'monospace'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildOutputDetailRow(String label, String value, {bool mono = false}) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 100,
            child: Text(
              '$label:',
              style: const TextStyle(
                fontWeight: FontWeight.w500,
                fontSize: 11,
                color: Colors.black54,
              ),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: TextStyle(
                fontSize: 11,
                fontFamily: mono ? 'monospace' : null,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildSortButton(String label, String sortKey) {
    final isActive = _sortBy == sortKey;
    final arrow = isActive ? (_sortAscending ? ' ↑' : ' ↓') : '';
    return InkWell(
      onTap: () {
        setState(() {
          if (_sortBy == sortKey) {
            _sortAscending = !_sortAscending;
          } else {
            _sortBy = sortKey;
            _sortAscending = false;
          }
        });
      },
      borderRadius: BorderRadius.circular(4),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: isActive ? Colors.blue.shade100 : Colors.grey.shade200,
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(
          '$label$arrow',
          style: TextStyle(
            fontSize: 12,
            fontWeight: isActive ? FontWeight.bold : FontWeight.normal,
          ),
        ),
      ),
    );
  }

  List<OwnedOutput> _sortedOutputs() {
    final filtered = _allOutputs.where((o) => _showSpentOutputs || !o.spent).toList();

    filtered.sort((a, b) {
      int comparison;
      if (_sortBy == 'confirms') {
        final aConf = _currentHeight - a.blockHeight.toInt();
        final bConf = _currentHeight - b.blockHeight.toInt();
        comparison = aConf.compareTo(bConf);
      } else {
        comparison = a.amount.toInt().compareTo(b.amount.toInt());
      }
      return _sortAscending ? comparison : -comparison;
    });

    return filtered;
  }

  Widget _buildSelectButton(String label, VoidCallback onPressed) {
    return InkWell(
      onTap: onPressed,
      borderRadius: BorderRadius.circular(4),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: Colors.grey.shade200,
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(
          label,
          style: const TextStyle(fontSize: 12),
        ),
      ),
    );
  }

  void _selectAllSpendable() {
    setState(() {
      for (var output in _allOutputs) {
        if (output.spent) continue;
        final outputHeight = output.blockHeight.toInt();
        final confirmations = outputHeight > 0 ? _currentHeight - outputHeight : 0;
        if (confirmations >= 10) {
          final outputKey = '${output.txHash}:${output.outputIndex}';
          _selectedOutputs.add(outputKey);
        }
      }
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
      builder: (context) => _SaveWalletDialog(
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
          title: const Text('⚠️ Security Warning'),
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

    // Prepare wallet data to save
    final walletData = {
      'version': 0,
      'walletId': _walletId,
      'seed': _controller.text.trim(),
      'network': _network,
      'address': _derivedAddress,
      'outputs': _allOutputs.map((o) => {
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
      }).toList(),
      'scanState': {
        'isContinuousScanning': _isContinuousScanning,
        'isContinuousPaused': _isContinuousPaused,
        'continuousScanCurrentHeight': _continuousScanCurrentHeight,
        'continuousScanTargetHeight': _continuousScanTargetHeight,
        'isSynced': _isSynced,
        'daemonHeight': _daemonHeight,
      },
      'selectedOutputs': _selectedOutputs.toList(),
      'nodeUrl': _nodeUrlController.text,
    };

    final jsonString = jsonEncode(walletData);

    // Wait for encrypted response
    final completer = Completer<bool>();
    final subscription = WalletDataSavedResponse.rustSignalStream.listen((signal) {
      debugPrint('[SAVE] Received save response - success: ${signal.message.success}');
      if (!completer.isCompleted) {
        if (signal.message.success && signal.message.encryptedData != null) {
          // Store the encrypted data to localStorage with wallet-specific key
          debugPrint('[SAVE] Storing encrypted data to localStorage key: $_storageKey (${signal.message.encryptedData!.length} chars)');
          html.window.localStorage[_storageKey] = signal.message.encryptedData!;
          // Also store the active wallet ID
          html.window.localStorage['monero_active_wallet'] = _walletId;
          debugPrint('[SAVE] Data successfully written to localStorage for wallet: $_walletId');
          completer.complete(true);
        } else {
          debugPrint('[SAVE] Failed - no encrypted data in response');
          completer.complete(false);
        }
      }
    });

    // Send save request
    debugPrint('[SAVE] Sending SaveWalletDataRequest to Rust...');
    SaveWalletDataRequest(
      password: password,
      walletDataJson: jsonString,
    ).sendSignalToRust();

    final success = await completer.future.timeout(
      const Duration(seconds: 10),
      onTimeout: () {
        debugPrint('[SAVE] Timeout waiting for save response');
        return false;
      },
    );

    await subscription.cancel();

    setState(() {
      _isSaving = false;
      if (success) {
        _saveError = null;
        _lastSaveTime = DateTime.now().toString().substring(0, 19);
        debugPrint('[SAVE] Save completed successfully');
      } else {
        _saveError = 'Failed to save wallet data';
        debugPrint('[SAVE] Save failed with error: $_saveError');
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
      debugPrint('[SAVE] Showing error snackbar');
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Save failed: $_saveError'),
          backgroundColor: Colors.red,
          duration: const Duration(seconds: 3),
        ),
      );
    }
  }

  /// Scan localStorage for all available wallet IDs
  void _refreshAvailableWallets() {
    debugPrint('[WALLET] Scanning localStorage for available wallets...');
    final walletIds = <String>[];

    // Scan all localStorage keys - only include wallets that have saved data
    for (var i = 0; i < html.window.localStorage.length; i++) {
      final key = html.window.localStorage.keys.elementAt(i);
      if (key.startsWith('monero_wallet_')) {
        final walletId = key.substring('monero_wallet_'.length);
        walletIds.add(walletId);
      }
    }

    walletIds.sort();

    setState(() {
      _availableWalletIds = walletIds;
      // If current wallet isn't in the list and there are wallets, select the first one
      if (walletIds.isNotEmpty && !walletIds.contains(_walletId)) {
        _walletId = walletIds.first;
      }
    });

    debugPrint('[WALLET] Found ${walletIds.length} wallets: ${walletIds.join(', ')}');
  }

  /// Start a new wallet by clearing the current state
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

  /// Switch to a different wallet
  Future<void> _switchWallet(String newWalletId) async {
    if (newWalletId == _walletId) {
      debugPrint('[WALLET] Already on wallet: $newWalletId');
      return;
    }

    debugPrint('[WALLET] Switching from $_walletId to $newWalletId');

    // Clear current state
    setState(() {
      _walletId = newWalletId;
      _controller.text = '';
      _derivedAddress = null;
      _secretSpendKey = null;
      _secretViewKey = null;
      _publicSpendKey = null;
      _publicViewKey = null;
      _allOutputs = [];
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

    // Refresh available wallets
    _refreshAvailableWallets();

    // Auto-load if wallet has saved data
    if (html.window.localStorage.containsKey(_storageKey)) {
      debugPrint('[WALLET] Wallet $newWalletId has saved data, auto-loading...');
      await _loadWalletData();
    }
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

    // Get encrypted data from storage for this wallet
    debugPrint('[LOAD] Looking for wallet data at key: $_storageKey');
    final encryptedData = html.window.localStorage[_storageKey];
    if (encryptedData == null) {
      setState(() {
        _isLoadingWallet = false;
        _loadError = 'No stored wallet data found for wallet: $_walletId';
      });
      debugPrint('[LOAD] No data found at key: $_storageKey');
      return;
    }
    debugPrint('[LOAD] Found encrypted data (${encryptedData.length} chars)');

    // Wait for decrypted response
    final completer = Completer<String?>();
    final subscription = WalletDataLoadedResponse.rustSignalStream.listen((signal) {
      if (!completer.isCompleted) {
        if (signal.message.success && signal.message.walletDataJson != null) {
          completer.complete(signal.message.walletDataJson);
        } else {
          completer.complete(null);
        }
      }
    });

    // Send load request
    LoadWalletDataRequest(
      password: password,
      encryptedData: encryptedData,
    ).sendSignalToRust();

    final jsonString = await completer.future.timeout(
      const Duration(seconds: 10),
      onTimeout: () => null,
    );

    await subscription.cancel();

    if (jsonString == null) {
      setState(() {
        _isLoadingWallet = false;
        _loadError = 'Failed to decrypt wallet data (wrong password?)';
      });
      return;
    }

    try {
      final walletData = jsonDecode(jsonString) as Map<String, dynamic>;

      // Restore wallet state
      setState(() {
        // Restore seed and network
        _controller.text = walletData['seed'] as String? ?? '';
        _network = walletData['network'] as String? ?? 'stagenet';
        _derivedAddress = walletData['address'] as String?;
        _nodeUrlController.text = walletData['nodeUrl'] as String? ?? 'http://127.0.0.1:38081';

        // Restore outputs
        _allOutputs = (walletData['outputs'] as List).map((o) {
          final outputData = o as Map<String, dynamic>;
          return OwnedOutput(
            txHash: outputData['txHash'] as String,
            outputIndex: outputData['outputIndex'] as int,
            amount: Uint64(BigInt.parse(outputData['amount'] as String)),
            amountXmr: outputData['amountXmr'] as String,
            key: outputData['key'] as String,
            keyOffset: outputData['keyOffset'] as String,
            commitmentMask: outputData['commitmentMask'] as String,
            subaddressIndex: outputData['subaddressIndex'] != null
                ? Tuple2<int, int>(
                    outputData['subaddressIndex'][0] as int,
                    outputData['subaddressIndex'][1] as int,
                  )
                : null,
            paymentId: outputData['paymentId'] as String?,
            receivedOutputBytes: outputData['receivedOutputBytes'] as String,
            blockHeight: Uint64(BigInt.parse(outputData['blockHeight'] as String)),
            spent: outputData['spent'] as bool,
            keyImage: outputData['keyImage'] as String,
          );
        }).toList();

        // Restore scan state
        final scanState = walletData['scanState'] as Map<String, dynamic>;
        _isContinuousScanning = scanState['isContinuousScanning'] as bool;
        _isContinuousPaused = scanState['isContinuousPaused'] as bool;
        _continuousScanCurrentHeight = scanState['continuousScanCurrentHeight'] as int;
        _continuousScanTargetHeight = scanState['continuousScanTargetHeight'] as int;
        _isSynced = scanState['isSynced'] as bool;
        _daemonHeight = scanState['daemonHeight'] as int?;

        // Set block height field to resume scanning from last synced height
        if (_continuousScanCurrentHeight > 0) {
          _blockHeightController.text = _continuousScanCurrentHeight.toString();
          _blockHeightUserEdited = false;
        }

        // Restore selected outputs
        _selectedOutputs = Set<String>.from(walletData['selectedOutputs'] as List);

        _isLoadingWallet = false;
        _loadError = null;
      });

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
    } catch (e) {
      setState(() {
        _isLoadingWallet = false;
        _loadError = 'Failed to parse wallet data: $e';
      });
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
              debugPrint('[STORAGE] Clearing data for wallet: $deletedWalletId');
              html.window.localStorage.remove(_storageKey);
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

/// Dialog for saving wallet data with wallet ID and password
class _SaveWalletDialog extends StatefulWidget {
  final String initialWalletId;
  final List<String> existingWalletIds;

  const _SaveWalletDialog({
    required this.initialWalletId,
    required this.existingWalletIds,
  });

  @override
  State<_SaveWalletDialog> createState() => _SaveWalletDialogState();
}

class _SaveWalletDialogState extends State<_SaveWalletDialog> {
  late final TextEditingController _walletIdController;
  final _passwordController = TextEditingController();
  final _confirmPasswordController = TextEditingController();
  String? _error;
  bool _obscurePassword = true;
  bool _obscureConfirm = true;

  @override
  void initState() {
    super.initState();
    _walletIdController = TextEditingController(text: widget.initialWalletId);
  }

  @override
  void dispose() {
    _walletIdController.dispose();
    _passwordController.dispose();
    _confirmPasswordController.dispose();
    super.dispose();
  }

  void _submit() {
    final walletId = _walletIdController.text.trim();
    final password = _passwordController.text;

    // Validate wallet ID
    if (walletId.isEmpty) {
      setState(() {
        _error = 'Wallet ID is required';
      });
      return;
    }

    // Check for invalid characters
    if (!RegExp(r'^[a-zA-Z0-9_-]+$').hasMatch(walletId)) {
      setState(() {
        _error = 'Wallet ID can only contain letters, numbers, underscores, and dashes';
      });
      return;
    }

    // Validate password confirmation if password is not empty
    if (password.isNotEmpty) {
      final confirm = _confirmPasswordController.text;
      if (password != confirm) {
        setState(() {
          _error = 'Passwords do not match';
        });
        return;
      }
    }

    Navigator.of(context).pop({
      'walletId': walletId,
      'password': password,
    });
  }

  @override
  Widget build(BuildContext context) {
    final isOverwriting = widget.existingWalletIds.contains(_walletIdController.text.trim());

    return AlertDialog(
      title: const Text('Save Wallet Data'),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _walletIdController,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Wallet ID',
                hintText: 'e.g., my_wallet, savings',
                border: OutlineInputBorder(),
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) => _submit(),
            ),
            if (isOverwriting) ...[
              const SizedBox(height: 8),
              Container(
                padding: const EdgeInsets.all(8),
                decoration: BoxDecoration(
                  color: Colors.orange.shade50,
                  borderRadius: BorderRadius.circular(4),
                  border: Border.all(color: Colors.orange.shade200),
                ),
                child: Row(
                  children: [
                    Icon(Icons.warning, color: Colors.orange.shade700, size: 16),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        'This will overwrite existing wallet data',
                        style: TextStyle(color: Colors.orange.shade900, fontSize: 12),
                      ),
                    ),
                  ],
                ),
              ),
            ],
            const SizedBox(height: 16),
            TextField(
              controller: _passwordController,
              obscureText: _obscurePassword,
              decoration: InputDecoration(
                labelText: 'Password (optional)',
                border: const OutlineInputBorder(),
                suffixIcon: IconButton(
                  icon: Icon(_obscurePassword ? Icons.visibility : Icons.visibility_off),
                  onPressed: () => setState(() => _obscurePassword = !_obscurePassword),
                ),
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) {
                if (_passwordController.text.isEmpty) {
                  _submit();
                }
              },
            ),
            if (_passwordController.text.isNotEmpty) ...[
              const SizedBox(height: 16),
              TextField(
                controller: _confirmPasswordController,
                obscureText: _obscureConfirm,
                decoration: InputDecoration(
                  labelText: 'Confirm Password',
                  border: const OutlineInputBorder(),
                  suffixIcon: IconButton(
                    icon: Icon(_obscureConfirm ? Icons.visibility : Icons.visibility_off),
                    onPressed: () => setState(() => _obscureConfirm = !_obscureConfirm),
                  ),
                ),
                onSubmitted: (_) => _submit(),
              ),
            ],
            if (_error != null) ...[
              const SizedBox(height: 12),
              Text(
                _error!,
                style: TextStyle(color: Colors.red.shade700, fontSize: 12),
              ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(null),
          child: const Text('Cancel'),
        ),
        ElevatedButton(
          onPressed: _submit,
          child: const Text('Save'),
        ),
      ],
    );
  }
}
