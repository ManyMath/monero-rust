import 'package:tuple/tuple.dart';
import '../src/bindings/bindings.dart';
import './wallet_transaction.dart';

class WalletInstance {
  final String walletId;
  final String seed;
  final String network;
  final String address;

  List<OwnedOutput> outputs;
  List<WalletTransaction> transactions;
  int currentHeight;
  int daemonHeight;
  bool isScanning;
  bool isClosed;

  // Account management
  int activeAccount;
  List<int> accounts; // List of account indices
  Map<int, List<OwnedOutput>> outputsByAccount; // Account -> Outputs mapping

  WalletInstance({
    required this.walletId,
    required this.seed,
    required this.network,
    required this.address,
    this.outputs = const [],
    this.transactions = const [],
    this.currentHeight = 0,
    this.daemonHeight = 0,
    this.isScanning = false,
    this.isClosed = false,
    this.activeAccount = 0,
    List<int>? accounts,
    Map<int, List<OwnedOutput>>? outputsByAccount,
  })  : accounts = accounts ?? [0],
        outputsByAccount = outputsByAccount ?? {0: const []};
  // Get outputs for the active account (or all outputs if activeAccount == -1)
  List<OwnedOutput> get activeAccountOutputs {
    // If "All" is selected, return all outputs
    if (activeAccount == -1) {
      return outputs;
    }

    // Filter from outputs list by account index to support dynamically scanned outputs
    return outputs.where((output) {
      if (output.subaddressIndex == null) {
        // Outputs without subaddress belong to account 0
        return activeAccount == 0;
      }
      return output.subaddressIndex!.item1 == activeAccount;
    }).toList();
  }

  double get confirmedBalance {
    const minConfirmations = 10;
    double balance = 0.0;

    for (var output in activeAccountOutputs) {
      if (output.spent) continue;

      final confirmations = daemonHeight - output.blockHeight.toInt();
      if (confirmations >= minConfirmations) {
        balance += double.tryParse(output.amountXmr) ?? 0.0;
      }
    }

    return balance;
  }

  double get unconfirmedBalance {
    const minConfirmations = 10;
    double balance = 0.0;

    for (var output in activeAccountOutputs) {
      if (output.spent) continue;

      final confirmations = daemonHeight - output.blockHeight.toInt();
      if (confirmations < minConfirmations) {
        balance += double.tryParse(output.amountXmr) ?? 0.0;
      }
    }

    return balance;
  }

  double get totalBalance => confirmedBalance + unconfirmedBalance;

  List<OwnedOutput> get unspentOutputs =>
      activeAccountOutputs.where((o) => !o.spent).toList();

  List<OwnedOutput> get spentOutputs =>
      activeAccountOutputs.where((o) => o.spent).toList();

  WalletInstance copyWith({
    String? walletId,
    String? seed,
    String? network,
    String? address,
    List<OwnedOutput>? outputs,
    List<WalletTransaction>? transactions,
    int? currentHeight,
    int? daemonHeight,
    bool? isScanning,
    bool? isClosed,
    int? activeAccount,
    List<int>? accounts,
    Map<int, List<OwnedOutput>>? outputsByAccount,
  }) {
    return WalletInstance(
      walletId: walletId ?? this.walletId,
      seed: seed ?? this.seed,
      network: network ?? this.network,
      address: address ?? this.address,
      outputs: outputs ?? this.outputs,
      transactions: transactions ?? this.transactions,
      currentHeight: currentHeight ?? this.currentHeight,
      daemonHeight: daemonHeight ?? this.daemonHeight,
      isScanning: isScanning ?? this.isScanning,
      isClosed: isClosed ?? this.isClosed,
      activeAccount: activeAccount ?? this.activeAccount,
      accounts: accounts ?? this.accounts,
      outputsByAccount: outputsByAccount ?? this.outputsByAccount,
    );
  }

  /// Create a new account by adding it to the accounts list
  WalletInstance createAccount(int accountIndex) {
    if (accounts.contains(accountIndex)) {
      return this; // Account already exists
    }

    final newAccounts = List<int>.from(accounts)..add(accountIndex);
    newAccounts.sort(); // Keep accounts sorted

    final newOutputsByAccount = Map<int, List<OwnedOutput>>.from(outputsByAccount);
    newOutputsByAccount[accountIndex] = [];

    return copyWith(
      accounts: newAccounts,
      outputsByAccount: newOutputsByAccount,
    );
  }

  /// Switch to a different account (or -1 for "All" accounts)
  WalletInstance switchAccount(int accountIndex) {
    // Allow -1 for "All" accounts view
    if (accountIndex != -1 && !accounts.contains(accountIndex)) {
      throw ArgumentError('Account $accountIndex does not exist');
    }

    return copyWith(activeAccount: accountIndex);
  }

  /// Add an output to a specific account
  WalletInstance addOutputToAccount(int accountIndex, OwnedOutput output) {
    final newOutputsByAccount = Map<int, List<OwnedOutput>>.from(outputsByAccount);
    final accountOutputs = List<OwnedOutput>.from(newOutputsByAccount[accountIndex] ?? []);
    accountOutputs.add(output);
    newOutputsByAccount[accountIndex] = accountOutputs;

    // Also update the legacy outputs list if this is the active account
    final newOutputs = accountIndex == activeAccount
        ? (List<OwnedOutput>.from(outputs)..add(output))
        : outputs;

    return copyWith(
      outputsByAccount: newOutputsByAccount,
      outputs: newOutputs,
    );
  }

  WalletConfig toWalletConfig() {
    // Calculate the highest account index for lookahead
    final highestAccount = accounts.isEmpty ? 0 : accounts.reduce((a, b) => a > b ? a : b);

    return WalletConfig(
      seed: seed,
      network: network,
      accountLookahead: highestAccount,
    );
  }

  Map<String, dynamic> toJson() => {
    'walletId': walletId,
    'seed': seed,
    'network': network,
    'address': address,
    'outputs': outputs.map((o) => {
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
      'isCoinbase': o.isCoinbase,
    }).toList(),
    'transactions': transactions.map((t) => t.toJson()).toList(),
    'currentHeight': currentHeight,
    'daemonHeight': daemonHeight,
    'isScanning': isScanning,
    'isClosed': isClosed,
    'activeAccount': activeAccount,
    'accounts': accounts,
    'outputsByAccount': outputsByAccount.map((accountIndex, accountOutputs) {
      return MapEntry(
        accountIndex.toString(),
        accountOutputs.map((o) => {
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
          'isCoinbase': o.isCoinbase,
        }).toList(),
      );
    }),
  };

  factory WalletInstance.fromJson(Map<String, dynamic> json) {
    return WalletInstance(
      walletId: json['walletId'] as String,
      seed: json['seed'] as String,
      network: json['network'] as String,
      address: json['address'] as String,
      outputs: (json['outputs'] as List).map((o) {
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
          isCoinbase: (outputData['isCoinbase'] as bool?) ?? false,
        );
      }).toList(),
      transactions: (json['transactions'] as List? ?? [])
          .map((t) => WalletTransaction.fromJson(t as Map<String, dynamic>))
          .toList(),
      currentHeight: json['currentHeight'] as int,
      daemonHeight: json['daemonHeight'] as int,
      isScanning: json['isScanning'] as bool,
      isClosed: json['isClosed'] as bool,
      activeAccount: json['activeAccount'] as int,
      accounts: (json['accounts'] as List).cast<int>(),
      outputsByAccount: (json['outputsByAccount'] as Map<String, dynamic>).map(
        (key, value) {
          final accountIndex = int.parse(key);
          final accountOutputs = (value as List).map((o) {
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
              isCoinbase: (outputData['isCoinbase'] as bool?) ?? false,
            );
          }).toList();
          return MapEntry(accountIndex, accountOutputs);
        },
      ),
    );
  }
}
