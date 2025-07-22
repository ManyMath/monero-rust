import 'package:flutter/material.dart';
import 'package:monero_extension/utils/network_utils.dart';
import '../src/ffi/signal_types.dart';
import '../utils/key_parser.dart';
import '../utils/output_utils.dart';
import '../models/wallet_transaction.dart';
import '../services/wallet_scan_service.dart';
import '../services/wallet_polling_service.dart';
import '../widgets/scanning_panel.dart' show NodeConnectionState;
import '../src/ffi/signal_hub.dart';
import 'wallet_state.dart';

enum LookaheadMode {
  none('No lookahead', 0, 0),
  hardware('Hardware wallet', 5, 20),
  compatibility('Compatibility', 50, 200);

  final String label;
  final int accounts;
  final int subaddresses;
  const LookaheadMode(this.label, this.accounts, this.subaddresses);
}

class ScanState extends ChangeNotifier {
  final WalletState _walletState;
  final WalletPollingService pollingService;

  LookaheadMode lookaheadMode = LookaheadMode.none;

  bool isScanning = false;
  bool isContinuousScanning = false;
  bool isContinuousPaused = false;
  bool isSynced = false;
  bool isScanningMempool = false;
  bool hasConnectedOnce = false;
  int continuousScanTargetHeight = 0;
  int? daemonHeight;
  BlockScanResponse? scanResult;
  String? scanError;

  ReorgDetectedResponse? reorgInfo;
  List<DoubleSpendConflict>? doubleSpendConflicts;

  int get currentHeight => daemonHeight ?? scanResult?.blockHeight ?? 0;
  int get continuousScanCurrentHeight => _walletState.continuousScanCurrentHeight;

  NodeConnectionState get connectionState {
    if (scanError != null && !isContinuousScanning && !isScanning) {
      return NodeConnectionState.disconnected;
    }
    if (isScanning && !isContinuousScanning) {
      return NodeConnectionState.connecting;
    }
    if (isContinuousScanning && !isSynced) {
      return NodeConnectionState.synchronizing;
    }
    if (isSynced || hasConnectedOnce) {
      return NodeConnectionState.synchronized;
    }
    return NodeConnectionState.disconnected;
  }

  ScanState({
    required WalletState walletState,
    required WalletPollingService pollingService,
    required SignalHub signalHub,
  })  : _walletState = walletState,
        pollingService = pollingService {
    signalHub.onBlockScan = _handleBlockScan;
    signalHub.onDaemonHeight = _handleDaemonHeight;
    signalHub.onSyncProgress = _handleSyncProgress;
    signalHub.onSpentStatusUpdated = _handleSpentStatusUpdated;
    signalHub.onMempoolScan = _handleMempoolScan;
    signalHub.onMultiWalletScan = _handleMultiWalletScan;
    signalHub.onReorgDetected = _handleReorgDetected;
    signalHub.onDoubleSpendDetected = _handleDoubleSpendDetected;
    signalHub.onTransactionStatusUpdate = _handleTransactionStatusUpdate;

    _walletState.checkIsContinuousScanning = () => isContinuousScanning;
    _walletState.onWalletStateCleared = _onWalletStateCleared;
    _walletState.onDaemonHeightRestored = _onDaemonHeightRestored;
  }

  void setLookaheadMode(LookaheadMode mode) {
    lookaheadMode = mode;
    notifyListeners();
  }

  void _onWalletStateCleared() {
    daemonHeight = null;
    scanResult = null;
    scanError = null;
    isSynced = false;
    hasConnectedOnce = false;
    continuousScanTargetHeight = 0;
    notifyListeners();
  }

  void _onDaemonHeightRestored(int? height) {
    daemonHeight = height;
    notifyListeners();
  }

  // Signal handlers
  void _handleBlockScan(BlockScanResponse msg) {
    if (_walletState.isChangingSeed) return;

    isScanning = false;
    if (msg.success) {
      scanResult = msg;
      scanError = null;
      daemonHeight = msg.daemonHeight;
      hasConnectedOnce = true;
      _walletState.lifecycle.integrateSingleBlockScanResults(msg);
      _walletState.deriveSubaddresses();
      _walletState.notify();
    } else {
      scanResult = null;
      scanError = msg.error ?? 'Unknown error during scan';
    }
    notifyListeners();
  }

  void _handleDaemonHeight(DaemonHeightResponse msg) {
    if (msg.success) {
      daemonHeight = msg.daemonHeight;
      hasConnectedOnce = true;
    } else {
      scanError = msg.error ?? 'Failed to get daemon height';
    }
    notifyListeners();
  }

  void _handleSyncProgress(SyncProgressResponse msg) {
    if (_walletState.isChangingSeed) {
      if (!msg.isScanning) {
        _walletState.isChangingSeed = false;
        isContinuousScanning = false;
        _walletState.clearWalletState();
        continuousScanTargetHeight = 0;
        isSynced = false;
        daemonHeight = null;
        scanResult = null;
        notifyListeners();
      }
      return;
    }

    final wasSynced = isSynced;
    final wasScanning = isContinuousScanning;

    _walletState.continuousScanCurrentHeight = msg.currentHeight;
    continuousScanTargetHeight = msg.daemonHeight;
    isSynced = msg.isSynced;
    if (!isContinuousPaused) {
      isContinuousScanning = msg.isScanning;
    } else if (!msg.isScanning) {
      isContinuousScanning = false;
    }
    if (isContinuousScanning && !wasScanning) {
      isContinuousPaused = false;
    }
    if (!_walletState.blockHeightFocusNode.hasFocus) {
      _walletState.blockHeightController.text = _walletState.continuousScanCurrentHeight.toString();
      _walletState.blockHeightUserEdited = false;
    }

    notifyListeners();

    if (isContinuousScanning && !wasScanning) {
      stopPollingTimers();
    }
    if (!isContinuousScanning && (wasScanning || (isSynced && !wasSynced))) {
      startPollingTimers();
    }

    if (!msg.isScanning && _walletState.onScanStopped != null) {
      final cb = _walletState.onScanStopped!;
      _walletState.onScanStopped = null;
      cb();
    }
  }

  void _handleSpentStatusUpdated(SpentStatusUpdatedResponse msg) {
    _walletState.pendingSpentKeyImages.removeAll(msg.spentKeyImages);
    for (var wallet in _walletState.lifecycle.openWallets.values) {
      OutputUtils.markSpentByKeyImages(wallet.outputs, msg.spentKeyImages, _walletState.selectedOutputs);
    }
    OutputUtils.markSpentByKeyImages(_walletState.allOutputs, msg.spentKeyImages, _walletState.selectedOutputs);
    _walletState.notify();
    notifyListeners();
  }

  void _handleMempoolScan(MempoolScanResponse msg) {
    if (_walletState.isChangingSeed) return;

    isScanningMempool = false;
    if (msg.success) {
      OutputUtils.addIfAbsent(_walletState.allOutputs, msg.outputs);
      OutputUtils.markSpentByKeyImages(_walletState.allOutputs, msg.spentKeyImages, _walletState.selectedOutputs);
      _walletState.ensureAccountsExistForOutputs(msg.outputs);

      if (msg.outputs.isNotEmpty) {
        final existingTxHashes = <String>{
          for (var tx in _walletState.allTransactions) tx.txHash,
        };
        final outputsByTx = <String, List<OwnedOutput>>{};
        for (var output in msg.outputs) {
          outputsByTx.putIfAbsent(output.txHash, () => []).add(output);
        }
        final newTxs = <WalletTransaction>[];
        for (var entry in outputsByTx.entries) {
          if (!existingTxHashes.contains(entry.key)) {
            newTxs.add(WalletTransaction(
              txHash: entry.key,
              blockHeight: 0,
              blockTimestamp: 0,
              receivedOutputs: entry.value,
              spentKeyImages: [],
            ));
          }
        }
        if (newTxs.isNotEmpty) {
          _walletState.allTransactions = [
            ..._walletState.allTransactions,
            ...newTxs,
          ];
        }
      }

      _walletState.deriveSubaddresses();
      _walletState.notify();
    }
    notifyListeners();
  }

  void _handleMultiWalletScan(MultiWalletScanResponse msg) {
    if (_walletState.isChangingSeed) return;

    if (!msg.success) {
      scanError = msg.error;
      notifyListeners();
      return;
    }

    daemonHeight = msg.daemonHeight;
    _walletState.lifecycle.distributeMultiWalletScanResults(
      walletResults: msg.walletResults,
      blockHeight: msg.blockHeight,
      daemonHeight: msg.daemonHeight,
      spentKeyImages: msg.spentKeyImages,
      spentKeyImageTxHashes: msg.spentKeyImageTxHashes,
      blockTimestamp: msg.blockTimestamp,
    );
    _walletState.deriveSubaddresses();
    _walletState.notify();
    notifyListeners();
    _walletState.updateBlockHeightFromWallets();
  }

  void _handleReorgDetected(ReorgDetectedResponse msg) {
    reorgInfo = msg;
    _walletState.lifecycle.handleReorgDetected(
      splitHeight: msg.splitHeight,
      removedKeyImages: msg.removedKeyImages,
      unspentKeyImages: msg.unspentKeyImages,
    );
    if (_walletState.lifecycle.activeWallet != null) {
      _walletState.allOutputs = List.from(_walletState.lifecycle.activeWallet!.outputs);
      _walletState.allTransactions = List.from(_walletState.lifecycle.activeWallet!.transactions);
    }
    _walletState.notify();
    notifyListeners();
  }

  void _handleDoubleSpendDetected(DoubleSpendDetectedResponse msg) {
    final incoming = msg.conflicts;
    if (doubleSpendConflicts == null) {
      doubleSpendConflicts = List.from(incoming);
    } else {
      doubleSpendConflicts!.addAll(incoming);
    }
    notifyListeners();
  }

  void _handleTransactionStatusUpdate(TransactionStatusUpdate msg) {
    if (msg.status == 'confirmed' && msg.confirmedHeight != null) {
      final confirmedHeight = msg.confirmedHeight!;

      final txList = _walletState.allTransactions;
      final idx = txList.indexWhere((tx) => tx.txHash == msg.txId);
      if (idx != -1 && txList[idx].blockHeight == 0) {
        _walletState.pendingSpentKeyImages.removeAll(txList[idx].spentKeyImages);

        final updated = txList[idx].copyWith(
          blockHeight: confirmedHeight,
          blockTimestamp: DateTime.now().millisecondsSinceEpoch ~/ 1000,
        );
        _walletState.allTransactions = [
          ...txList.sublist(0, idx),
          updated,
          ...txList.sublist(idx + 1),
        ];
      }

      final outputs = _walletState.allOutputs;
      for (int i = 0; i < outputs.length; i++) {
        final o = outputs[i];
        if (o.txHash == msg.txId && o.blockHeight == 0) {
          outputs[i] = OwnedOutput(
            txHash: o.txHash,
            outputIndex: o.outputIndex,
            amount: o.amount,
            amountXmr: o.amountXmr,
            key: o.key,
            keyOffset: o.keyOffset,
            commitmentMask: o.commitmentMask,
            subaddressIndex: o.subaddressIndex,
            paymentId: o.paymentId,
            receivedOutputBytes: o.receivedOutputBytes,
            blockHeight: confirmedHeight,
            spent: o.spent,
            keyImage: o.keyImage,
            isCoinbase: o.isCoinbase,
            frozen: o.frozen,
          );
        }
      }

      _walletState.notify();
      notifyListeners();
    }
  }

  // Methods
  void scanBlock() {
    final validation = WalletScanService.validateScanBlock(
      seed: _walletState.seedController.text,
      blockHeight: _walletState.blockHeightController.text,
      nodeUrl: _walletState.nodeUrlController.text,
    );

    if (!validation.isValid) {
      scanError = validation.error;
      notifyListeners();
      return;
    }

    isScanning = true;
    scanResult = null;
    scanError = null;
    notifyListeners();

    WalletScanService.scanBlock(
      seed: validation.normalizedSeed!,
      blockHeight: validation.blockHeight!,
      nodeUrl: validation.nodeUrl!,
      network: _walletState.network,
    );
  }

  void startContinuousScan() {
    final walletsToScan = _walletState.activeWallets;

    final validation = WalletScanService.validateContinuousScan(
      seed: _walletState.seedController.text,
      blockHeight: _walletState.blockHeightController.text,
      nodeUrl: _walletState.nodeUrlController.text,
      activeWallets: walletsToScan,
    );

    if (!validation.isValid) {
      scanError = validation.error;
      notifyListeners();
      return;
    }

    final wasScanning = isContinuousScanning;
    if (wasScanning) {
      WalletScanService.pauseContinuousScan();
      isContinuousScanning = false;
      notifyListeners();
    }

    void doStartScan() {
      stopPollingTimers();

      scanError = null;
      isContinuousPaused = false;
      isContinuousScanning = true;

      for (var wallet in walletsToScan) {
        wallet.isScanning = true;
      }
      notifyListeners();

      final result = walletsToScan.isEmpty ? KeyParser.parse(_walletState.seedController.text) : null;
      int highestAccount = 0;
      final accountSources = walletsToScan.isEmpty
          ? [_walletState.accounts]
          : walletsToScan.map((w) => w.accounts);
      for (final accts in accountSources) {
        if (accts.isNotEmpty) {
          final max = accts.reduce((a, b) => a > b ? a : b);
          if (max > highestAccount) highestAccount = max;
        }
      }
      WalletScanService.startContinuousScan(
        nodeUrl: validation.nodeUrl!,
        startHeight: validation.startHeight!,
        walletsToScan: walletsToScan,
        seed: result?.normalizedInput,
        network: walletsToScan.isEmpty ? _walletState.network : null,
        accountLookahead: highestAccount + lookaheadMode.accounts,
        subaddressLookahead: lookaheadMode.subaddresses,
      );
    }

    if (wasScanning) {
      _walletState.onScanStopped = doStartScan;
    } else {
      doStartScan();
    }
  }

  void pauseContinuousScan() {
    isContinuousPaused = true;
    isContinuousScanning = false;
    notifyListeners();
    WalletScanService.pauseContinuousScan();
    startPollingTimers();
  }

  void scanMempool() {
    final validation = WalletScanService.validateMempoolScan(
      seed: _walletState.seedController.text,
      nodeUrl: _walletState.nodeUrlController.text,
    );

    if (!validation.isValid) {
      scanError = validation.error;
      notifyListeners();
      return;
    }

    isScanningMempool = true;
    scanError = null;
    notifyListeners();

    final highestAccount = _walletState.accounts.isEmpty ? 0 : _walletState.accounts.reduce((a, b) => a > b ? a : b);
    WalletScanService.scanMempool(
      seed: validation.normalizedSeed!,
      nodeUrl: validation.nodeUrl!,
      network: _walletState.network,
      accountLookahead: highestAccount + lookaheadMode.accounts,
      subaddressLookahead: lookaheadMode.subaddresses,
    );
  }

  String continuousScanButtonLabel() {
    if (isContinuousScanning) return 'Pause Scan';
    if (isContinuousPaused) {
      final currentText = _walletState.blockHeightController.text.trim();
      final height = int.tryParse(currentText);
      if (_walletState.blockHeightUserEdited && height != null && height > _walletState.continuousScanCurrentHeight) {
        return 'Start Skipscan';
      }
      return 'Start Rescan';
    }
    return 'Start Scan';
  }

  Color continuousScanButtonColor() {
    if (isContinuousScanning) return Colors.orange;
    if (isContinuousPaused) return Colors.blueGrey;
    return Colors.green;
  }

  void startPollingTimers() {
    pollingService.startPolling(
      onBlockRefresh: _onBlockRefreshTimer,
      onMempoolPoll: _onMempoolPollTimer,
    );
  }

  void stopPollingTimers() {
    pollingService.stopPolling();
  }

  void _onBlockRefreshTimer() {
    if (isContinuousPaused || isContinuousScanning) return;

    final nodeUrl = NetworkUtils.normalizeNodeUrl(_walletState.nodeUrlController.text);
    final walletsToScan = _walletState.activeWallets;

    if (walletsToScan.isEmpty) return;

    WalletScanService.queryDaemonHeight(nodeUrl);

    if (walletsToScan.length > 1) {
      final walletConfigs = walletsToScan.map((w) => w.toWalletConfig(subaddressLookahead: lookaheadMode.subaddresses)).toList();
      StartMultiWalletScanRequest(
        nodeUrl: nodeUrl,
        startHeight: _walletState.continuousScanCurrentHeight,
        wallets: walletConfigs,
      ).sendSignalToRust();
    } else {
      final wallet = walletsToScan.first;
      final walletAccounts = wallet.accounts;
      final highestAccount = walletAccounts.isEmpty ? 0 : walletAccounts.reduce((a, b) => a > b ? a : b);
      StartContinuousScanRequest(
        nodeUrl: nodeUrl,
        startHeight: _walletState.continuousScanCurrentHeight,
        seed: wallet.seed,
        network: wallet.network,
        accountLookahead: highestAccount + lookaheadMode.accounts,
        subaddressLookahead: lookaheadMode.subaddresses,
        passphrase: '',
        bip39AccountIndex: 0,
      ).sendSignalToRust();
    }
  }

  void _onMempoolPollTimer() {
    final seed = _walletState.seedController.text.trim();
    if (seed.isEmpty) return;

    final nodeUrl = NetworkUtils.normalizeNodeUrl(_walletState.nodeUrlController.text);
    final highestAccount = _walletState.accounts.isEmpty ? 0 : _walletState.accounts.reduce((a, b) => a > b ? a : b);

    MempoolScanRequest(
      nodeUrl: nodeUrl,
      seed: seed,
      network: _walletState.network,
      accountLookahead: highestAccount + lookaheadMode.accounts,
      subaddressLookahead: lookaheadMode.subaddresses,
      passphrase: '',
      bip39AccountIndex: 0,
    ).sendSignalToRust();
  }
}
