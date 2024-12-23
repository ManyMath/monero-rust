import '../src/bindings/bindings.dart';
import 'wallet_transaction.dart';

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
  });
  double get confirmedBalance {
    const minConfirmations = 10;
    double balance = 0.0;

    for (var output in outputs) {
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

    for (var output in outputs) {
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
      outputs.where((o) => !o.spent).toList();

  List<OwnedOutput> get spentOutputs =>
      outputs.where((o) => o.spent).toList();

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
    );
  }

  WalletConfig toWalletConfig() {
    return WalletConfig(
      seed: seed,
      network: network,
    );
  }
}
