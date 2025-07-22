import 'package:flutter/material.dart';
import '../src/ffi/signal_types.dart';
import '../utils/output_utils.dart';
import '../utils/output_lock_utils.dart';
import '../models/wallet_transaction.dart';
import '../services/transaction_service.dart';
import '../src/ffi/signal_hub.dart';
import 'wallet_state.dart';
import 'output_state.dart';
import 'scan_state.dart';

class TransactionState extends ChangeNotifier {
  final WalletState _walletState;
  final OutputState _outputState;
  final ScanState _scanState;

  final List<TextEditingController> destinationControllers = [TextEditingController()];
  final List<TextEditingController> amountControllers = [TextEditingController()];

  bool isCreatingTx = false;
  TransactionCreatedResponse? txResult;
  String? txError;

  bool isBroadcasting = false;
  TransactionBroadcastResponse? broadcastResult;
  String? broadcastError;
  bool isBroadcastRetryable = false;
  bool isBroadcastDoubleSpend = false;

  bool subtractFee = false;

  VoidCallback? onBroadcastSuccess;

  TransactionState({
    required WalletState walletState,
    required OutputState outputState,
    required ScanState scanState,
    required SignalHub signalHub,
  })  : _walletState = walletState,
        _outputState = outputState,
        _scanState = scanState {
    signalHub.onTransactionCreated = _handleTransactionCreated;
    signalHub.onTransactionBroadcast = _handleTransactionBroadcast;
  }

  void _handleTransactionCreated(TransactionCreatedResponse msg) {
    isCreatingTx = false;
    if (msg.success) {
      txResult = msg;
      txError = null;
      broadcastResult = null;
      broadcastError = null;

      final changeOwned = msg.changeOutputs.map(OutputUtils.changeOutputToOwned).toList();
      OutputUtils.addIfAbsent(_walletState.allOutputs, changeOwned);
      _walletState.notify();
    } else {
      txResult = null;
      txError = msg.error ?? 'Unknown error during transaction creation';
    }
    notifyListeners();
  }

  void _handleTransactionBroadcast(TransactionBroadcastResponse msg) {
    isBroadcasting = false;
    if (msg.success) {
      broadcastResult = msg;
      broadcastError = null;
      final thisTxSpentKeyImages = <String>[];
      if (txResult != null) {
        final spentKeys = txResult!.spentOutputHashes.toSet();
        for (final output in _walletState.allOutputs) {
          final key = '${output.txHash}:${output.outputIndex}';
          if (spentKeys.contains(key)) {
            thisTxSpentKeyImages.add(output.keyImage);
            _walletState.pendingSpentKeyImages.add(output.keyImage);
            _walletState.selectedOutputs.remove(key);
          }
        }
      }
      if (txResult != null) {
        final txId = txResult!.txId;
        final alreadyExists = _walletState.allTransactions.any((tx) => tx.txHash == txId);
        if (!alreadyExists) {
          final ownedOutputs = _walletState.allOutputs.where((o) =>
            o.txHash == txId && o.blockHeight == 0).toList();
          _walletState.allTransactions = [
            ..._walletState.allTransactions,
            WalletTransaction(
              txHash: txId,
              blockHeight: 0,
              blockTimestamp: 0,
              receivedOutputs: ownedOutputs,
              spentKeyImages: thisTxSpentKeyImages,
            ),
          ];
        }
      }
      _walletState.notify();
      onBroadcastSuccess?.call();
      for (var c in destinationControllers) {
        c.clear();
      }
      for (var c in amountControllers) {
        c.clear();
      }
    } else {
      broadcastResult = null;
      broadcastError = msg.error ?? 'Unknown error during broadcast';
      isBroadcastRetryable = msg.isRetryable;
      isBroadcastDoubleSpend = msg.isDoubleSpend;
    }
    notifyListeners();
  }

  String? checkMultiAccountOutputs() {
    final spendableAccounts = <int>{};
    for (final output in _walletState.allOutputs) {
      if (!output.spent && !output.frozen) {
        final account = output.subaddressIndex?.$1 ?? 0;
        spendableAccounts.add(account);
      }
    }

    if (spendableAccounts.length > 1 && _walletState.activeAccount != -1) {
      return 'Unfrozen outputs span multiple accounts (${spendableAccounts.join(', ')}). Switch to "All" accounts view or freeze outputs from other accounts.';
    }
    return null;
  }

  String? getMultiAccountWarning() {
    if (_walletState.activeAccount != -1) return null;

    final spendableAccounts = <int>{};
    for (final output in _walletState.allOutputs) {
      if (!output.spent && !output.frozen) {
        final account = output.subaddressIndex?.$1 ?? 0;
        spendableAccounts.add(account);
      }
    }

    if (spendableAccounts.length > 1) {
      final accountsList = spendableAccounts.toList()..sort();
      return 'WARNING: Creating transaction with unfrozen outputs from multiple accounts (${accountsList.join(', ')}). This may reduce privacy.';
    }
    return null;
  }

  void _hydrateRustWalletActor() {
    final seed = _walletState.seedController.text.trim();
    if (seed.isEmpty) return;
    RestoreWalletDataRequest(
      seed: seed,
      network: _walletState.network,
      outputs: _walletState.allOutputs,
      daemonHeight: _scanState.daemonHeight ?? 0,
      currentHeight: _walletState.continuousScanCurrentHeight,
      passphrase: '',
      bip39AccountIndex: 0,
    ).sendSignalToRust();
  }

  void createTransaction() {
    final recipientInputs = List.generate(
      destinationControllers.length,
      (i) => RecipientInput(
        address: destinationControllers[i].text,
        amount: amountControllers[i].text,
      ),
    );

    final multiAccountCheck = checkMultiAccountOutputs();
    if (multiAccountCheck != null) {
      txError = multiAccountCheck;
      notifyListeners();
      return;
    }

    final selectedSet = _walletState.selectedOutputs.isNotEmpty ? _walletState.selectedOutputs : null;

    final validation = TransactionService.validateTransactionCreation(
      seed: _walletState.seedController.text,
      availableOutputs: _outputState.filteredOutputs,
      recipients: recipientInputs,
      nodeUrl: _walletState.nodeUrlController.text,
      selectedOutputs: selectedSet,
      currentHeight: _scanState.currentHeight,
    );

    if (!validation.isValid) {
      txError = validation.error;
      notifyListeners();
      return;
    }

    isCreatingTx = true;
    txResult = null;
    txError = null;
    broadcastResult = null;
    broadcastError = null;
    notifyListeners();

    _hydrateRustWalletActor();

    TransactionService.createTransaction(
      seed: validation.normalizedSeed!,
      network: _walletState.network,
      recipients: validation.recipients!,
      nodeUrl: validation.nodeUrl!,
      selectedOutputs: validation.selectedOutputs,
      subtractFee: subtractFee,
    );
  }

  void broadcastTransaction() {
    final validation = TransactionService.validateTransactionBroadcast(
      txResult: txResult,
      nodeUrl: _walletState.nodeUrlController.text,
    );

    if (!validation.isValid) {
      broadcastError = validation.error;
      notifyListeners();
      return;
    }

    isBroadcasting = true;
    broadcastResult = null;
    broadcastError = null;
    notifyListeners();

    final spentHashes = txResult!.spentOutputHashes.toSet();
    final spentKeyImages = _walletState.allOutputs
        .where((o) => spentHashes.contains('${o.txHash}:${o.outputIndex}'))
        .map((o) => o.keyImage)
        .where((ki) => ki.isNotEmpty)
        .toList();

    TransactionService.broadcastTransaction(
      nodeUrl: validation.nodeUrl!,
      txBlob: validation.txBlob!,
      spentOutputHashes: validation.spentOutputHashes!,
      txId: txResult!.txId,
      spentKeyImages: spentKeyImages,
    );
  }

  void handleSendMax(BuildContext context, int recipientIndex) {
    if (_walletState.activeAccount == -1) {
      showDialog(
        context: context,
        builder: (context) {
          return AlertDialog(
            title: const Text('Send Max from all accounts?'),
            content: const Text('You are sweeping multiple accounts.\n\nContinue?'),
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
    final outputKeys = _walletState.selectedOutputs.isNotEmpty
        ? _walletState.selectedOutputs
        : _outputState.filteredOutputs.map((o) => '${o.txHash}:${o.outputIndex}').toSet();

    int totalAtomic = 0;
    for (final output in _outputState.filteredOutputs) {
      final key = '${output.txHash}:${output.outputIndex}';
      if (outputKeys.contains(key) &&
          !output.spent &&
          OutputLockUtils.isOutputSpendable(
            output: output,
            currentHeight: _scanState.currentHeight,
          )) {
        totalAtomic += output.amount;
      }
    }
    final fullBalance = totalAtomic / 1e12;

    amountControllers[recipientIndex].text = fullBalance.toStringAsFixed(12);
    subtractFee = true;
    notifyListeners();
  }

  void addRecipient() {
    if (destinationControllers.length >= 15) return;
    destinationControllers.add(TextEditingController());
    amountControllers.add(TextEditingController());
    notifyListeners();
  }

  void removeRecipient(int index) {
    if (destinationControllers.length <= 1) return;
    destinationControllers[index].dispose();
    amountControllers[index].dispose();
    destinationControllers.removeAt(index);
    amountControllers.removeAt(index);
    notifyListeners();
  }

  void notify() => notifyListeners();

  @override
  void dispose() {
    for (var c in destinationControllers) {
      c.dispose();
    }
    for (var c in amountControllers) {
      c.dispose();
    }
    super.dispose();
  }
}
