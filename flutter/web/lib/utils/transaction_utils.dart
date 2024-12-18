import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';

class TransactionUtils {
  /// Update transaction list from scan results
  static List<WalletTransaction> updateTransactionsFromScan(
    List<WalletTransaction> allTransactions,
    BlockScanResponse scan,
  ) {
    final updatedTransactions = List<WalletTransaction>.from(allTransactions);
    final blockHeight = scan.blockHeight.toInt();
    final blockTimestamp = scan.blockTimestamp.toInt();

    // Group received outputs by transaction hash
    final outputsByTx = <String, List<OwnedOutput>>{};
    for (var output in scan.outputs) {
      outputsByTx.putIfAbsent(output.txHash, () => []).add(output);
    }

    // Create/update transactions for received outputs
    for (var entry in outputsByTx.entries) {
      final txHash = entry.key;
      final outputs = entry.value;

      final existingIndex = updatedTransactions.indexWhere((t) => t.txHash == txHash);
      if (existingIndex == -1) {
        // New transaction
        updatedTransactions.add(WalletTransaction(
          txHash: txHash,
          blockHeight: blockHeight,
          blockTimestamp: blockTimestamp,
          receivedOutputs: outputs,
          spentKeyImages: [],
        ));
      } else {
        // Update existing transaction with new outputs
        final existing = updatedTransactions[existingIndex];
        final updatedOutputs = [...existing.receivedOutputs];
        for (var output in outputs) {
          if (!updatedOutputs.any((o) =>
              o.txHash == output.txHash && o.outputIndex == output.outputIndex)) {
            updatedOutputs.add(output);
          }
        }
        updatedTransactions[existingIndex] = WalletTransaction(
          txHash: existing.txHash,
          blockHeight: existing.blockHeight,
          blockTimestamp: existing.blockTimestamp,
          receivedOutputs: updatedOutputs,
          spentKeyImages: existing.spentKeyImages,
        );
      }
    }

    // Record spent key images
    for (var keyImage in scan.spentKeyImages) {
      // Find which transaction this spend belongs to
      final spendTxHash = 'spend:$keyImage';
      final existingIndex = updatedTransactions.indexWhere((t) => t.txHash == spendTxHash);

      if (existingIndex == -1) {
        // Create a synthetic "spend" transaction
        updatedTransactions.add(WalletTransaction(
          txHash: spendTxHash,
          blockHeight: blockHeight,
          blockTimestamp: blockTimestamp,
          receivedOutputs: [],
          spentKeyImages: [keyImage],
        ));
      } else {
        // Add to existing spend transaction
        final existing = updatedTransactions[existingIndex];
        if (!existing.spentKeyImages.contains(keyImage)) {
          updatedTransactions[existingIndex] = WalletTransaction(
            txHash: existing.txHash,
            blockHeight: existing.blockHeight,
            blockTimestamp: existing.blockTimestamp,
            receivedOutputs: existing.receivedOutputs,
            spentKeyImages: [...existing.spentKeyImages, keyImage],
          );
        }
      }
    }

    return updatedTransactions;
  }

  /// Sort transactions by confirmations or amount
  static List<WalletTransaction> sortTransactions(
    List<WalletTransaction> transactions,
    List<OwnedOutput> allOutputs,
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
        final aAmount = a.balanceChange(allOutputs).abs();
        final bAmount = b.balanceChange(allOutputs).abs();
        comparison = aAmount.compareTo(bAmount);
      }
      return ascending ? comparison : -comparison;
    });

    return sorted;
  }

  /// Sort outputs by confirmations or value
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
