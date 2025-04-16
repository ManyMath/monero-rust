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

    // Build O(1) lookup: txHash → index in transactions list
    final txIndexMap = <String, int>{};
    for (int i = 0; i < transactions.length; i++) {
      txIndexMap[transactions[i].txHash] = i;
    }

    final outputsByTx = <String, List<OwnedOutput>>{};
    for (var output in scan.outputs) {
      outputsByTx.putIfAbsent(output.txHash, () => []).add(output);
    }

    for (var entry in outputsByTx.entries) {
      final txHash = entry.key;
      final outputs = entry.value;

      final existingIndex = txIndexMap[txHash];
      if (existingIndex == null) {
        txIndexMap[txHash] = transactions.length;
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
        // Build O(1) dedup set from existing outputs
        final outputKeys = <String>{
          for (var o in updatedOutputs) '${o.txHash}:${o.outputIndex}',
        };
        for (var output in outputs) {
          final key = '${output.txHash}:${output.outputIndex}';
          if (outputKeys.add(key)) {
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

    // Build O(1) lookup for spent key images across all transactions
    final spentKeyImageSet = <String>{};
    for (var t in transactions) {
      spentKeyImageSet.addAll(t.spentKeyImages);
    }

    for (var spentKeyImage in scan.spentKeyImages) {
      final spentOutput = keyImageMap[spentKeyImage];
      if (spentOutput == null) continue;

      if (!spentKeyImageSet.contains(spentKeyImage)) {
        final syntheticTxHash = 'spend:$spentKeyImage';
        txIndexMap[syntheticTxHash] = transactions.length;
        spentKeyImageSet.add(spentKeyImage);
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

    final Map<String, double> balanceCache = sortBy != 'confirms'
        ? {for (var tx in transactions) tx.txHash: tx.balanceChange(keyImageMap).abs()}
        : {};

    sorted.sort((a, b) {
      int comparison;
      if (sortBy == 'confirms') {
        final aConf = currentHeight - a.blockHeight;
        final bConf = currentHeight - b.blockHeight;
        comparison = aConf.compareTo(bConf);
      } else {
        comparison = balanceCache[a.txHash]!.compareTo(balanceCache[b.txHash]!);
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
