import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';

class TransactionUtils {
  static Map<String, OwnedOutput> buildKeyImageMap(List<OwnedOutput> outputs) {
    return {for (var o in outputs) o.keyImage: o};
  }

  static List<WalletTransaction> updateTransactionsFromScan(
    List<WalletTransaction> existingTransactions,
    BlockScanResponse scan,
    Map<String, OwnedOutput> keyImageMap,
  ) {
    final transactions = List<WalletTransaction>.from(existingTransactions);
    final blockHeight = scan.blockHeight.toInt();
    final blockTimestamp = scan.blockTimestamp.toInt();

    final outputsByTx = <String, List<OwnedOutput>>{};
    for (var output in scan.outputs) {
      outputsByTx.putIfAbsent(output.txHash, () => []).add(output);
    }

    for (var entry in outputsByTx.entries) {
      final txHash = entry.key;
      final outputs = entry.value;

      final existingIndex = transactions.indexWhere((t) => t.txHash == txHash);
      if (existingIndex == -1) {
        transactions.add(WalletTransaction(
          txHash: txHash,
          blockHeight: blockHeight,
          blockTimestamp: blockTimestamp,
          receivedOutputs: outputs,
          spentKeyImages: [],
        ));
      } else {
        final existing = transactions[existingIndex];
        final updatedOutputs = [...existing.receivedOutputs];
        for (var output in outputs) {
          if (!updatedOutputs.any((o) =>
              o.txHash == output.txHash && o.outputIndex == output.outputIndex)) {
            updatedOutputs.add(output);
          }
        }
        transactions[existingIndex] = WalletTransaction(
          txHash: txHash,
          blockHeight: existing.blockHeight > 0 ? existing.blockHeight : blockHeight,
          blockTimestamp: existing.blockTimestamp > 0 ? existing.blockTimestamp : blockTimestamp,
          receivedOutputs: updatedOutputs,
          spentKeyImages: existing.spentKeyImages,
        );
      }
    }

    for (var spentKeyImage in scan.spentKeyImages) {
      final spentOutput = keyImageMap[spentKeyImage];
      if (spentOutput == null) continue;

      final existingTx = transactions.where((t) =>
          t.spentKeyImages.contains(spentKeyImage)).firstOrNull;
      if (existingTx == null) {
        final syntheticTxHash = 'spend:$spentKeyImage';
        transactions.add(WalletTransaction(
          txHash: syntheticTxHash,
          blockHeight: blockHeight,
          blockTimestamp: blockTimestamp,
          receivedOutputs: [],
          spentKeyImages: [spentKeyImage],
        ));
      }
    }

    return transactions;
  }

  static List<WalletTransaction> sortTransactions(
    List<WalletTransaction> transactions,
    Map<String, OwnedOutput> keyImageMap,
    String sortBy,
    bool ascending,
    int currentHeight,
  ) {
    final sorted = List<WalletTransaction>.from(transactions);

    sorted.sort((a, b) {
      int comparison;
      if (sortBy == 'confirms') {
        final aConf = currentHeight - a.blockHeight;
        final bConf = currentHeight - b.blockHeight;
        comparison = aConf.compareTo(bConf);
      } else {
        final aAmount = a.balanceChange(keyImageMap).abs();
        final bAmount = b.balanceChange(keyImageMap).abs();
        comparison = aAmount.compareTo(bAmount);
      }
      return ascending ? comparison : -comparison;
    });

    return sorted;
  }

  static List<OwnedOutput> sortOutputs(
    List<OwnedOutput> outputs,
    String sortBy,
    bool ascending,
    int currentHeight,
    bool showSpent,
  ) {
    final filtered = outputs.where((o) => showSpent || !o.spent).toList();

    filtered.sort((a, b) {
      int comparison;
      if (sortBy == 'confirms') {
        final aConf = currentHeight - a.blockHeight.toInt();
        final bConf = currentHeight - b.blockHeight.toInt();
        comparison = aConf.compareTo(bConf);
      } else {
        comparison = a.amount.toInt().compareTo(b.amount.toInt());
      }
      return ascending ? comparison : -comparison;
    });

    return filtered;
  }
}
