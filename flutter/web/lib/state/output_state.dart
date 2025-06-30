import 'dart:html' as html;
import 'package:flutter/foundation.dart';
import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';
import '../utils/transaction_utils.dart';
import 'wallet_state.dart';

class OutputState extends ChangeNotifier {
  final WalletState _walletState;

  bool showSpentOutputs = false;
  String sortBy = 'confirms';
  bool sortAscending = false;
  String txSortBy = 'confirms';
  bool txSortAscending = false;
  Set<String> expandedTransactions = {};

  Map<String, OwnedOutput>? _cachedKeyImageMap;
  List<OwnedOutput>? _cachedFilteredOutputs;
  List<WalletTransaction>? _cachedFilteredTransactions;
  int? _cachedTotalStorageBytes;

  OutputState({required WalletState walletState}) : _walletState = walletState {
    _walletState.addListener(_onWalletStateChanged);
  }

  void _onWalletStateChanged() {
    invalidateCaches();
    notifyListeners();
  }

  // Accessor delegates
  Set<String> get selectedOutputs => _walletState.selectedOutputs;
  set selectedOutputs(Set<String> v) => _walletState.selectedOutputs = v;
  List<OwnedOutput> get allOutputsAllAccounts => _walletState.allOutputs;
  List<WalletTransaction> get allTransactionsAllAccounts => _walletState.allTransactions;

  // Computed getters
  Map<String, OwnedOutput> get keyImageMap {
    _cachedKeyImageMap ??= TransactionUtils.buildKeyImageMap(allOutputsAllAccounts);
    return _cachedKeyImageMap!;
  }

  List<OwnedOutput> get filteredOutputs {
    if (_cachedFilteredOutputs != null) return _cachedFilteredOutputs!;

    final account = _walletState.activeAccount;
    if (account == -1) {
      _cachedFilteredOutputs = allOutputsAllAccounts;
    } else {
      _cachedFilteredOutputs = allOutputsAllAccounts.where((output) {
        if (output.subaddressIndex == null) {
          return account == 0;
        }
        return output.subaddressIndex!.item1 == account;
      }).toList();
    }
    return _cachedFilteredOutputs!;
  }

  List<WalletTransaction> getFilteredTransactions(Map<String, OwnedOutput> kim) {
    if (_cachedFilteredTransactions != null) return _cachedFilteredTransactions!;

    final account = _walletState.activeAccount;
    if (account == -1) {
      _cachedFilteredTransactions = allTransactionsAllAccounts;
    } else {
      _cachedFilteredTransactions = allTransactionsAllAccounts.where((tx) {
        final hasReceivedOutputs = tx.receivedOutputs.any((output) {
          if (output.subaddressIndex == null) return account == 0;
          return output.subaddressIndex!.item1 == account;
        });
        final hasSpentOutputs = tx.spentKeyImages.any((keyImage) {
          final spentOutput = kim[keyImage];
          if (spentOutput == null) return false;
          if (spentOutput.subaddressIndex == null) return account == 0;
          return spentOutput.subaddressIndex!.item1 == account;
        });
        return hasReceivedOutputs || hasSpentOutputs;
      }).toList();
    }
    return _cachedFilteredTransactions!;
  }

  List<WalletTransaction> sortedTransactions(
    List<WalletTransaction> transactions,
    Map<String, OwnedOutput> kim,
    int currentHeight,
  ) {
    return TransactionUtils.sortTransactions(
      transactions,
      kim,
      txSortBy,
      txSortAscending,
      currentHeight,
    );
  }

  void invalidateCaches() {
    _cachedKeyImageMap = null;
    _cachedFilteredOutputs = null;
    _cachedFilteredTransactions = null;
  }

  void invalidateStorageBytesCache() {
    _cachedTotalStorageBytes = null;
  }

  int calculateTotalStorageBytes() {
    if (_cachedTotalStorageBytes != null) return _cachedTotalStorageBytes!;
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
    _cachedTotalStorageBytes = totalBytes;
    return totalBytes;
  }

  static String formatBytes(int bytes) {
    if (bytes >= 1048576) {
      return '${(bytes / 1048576).toStringAsFixed(2)} MiB';
    } else if (bytes >= 1024) {
      return '${(bytes / 1024).toStringAsFixed(2)} KiB';
    } else {
      return '$bytes bytes';
    }
  }

  void notify() => notifyListeners();

  @override
  void dispose() {
    _walletState.removeListener(_onWalletStateChanged);
    super.dispose();
  }
}
