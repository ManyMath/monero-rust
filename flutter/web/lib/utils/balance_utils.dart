import '../src/bindings/bindings.dart';
import 'output_lock_utils.dart';

class BalanceInfo {
  final double totalBalance;
  final double unlockedBalance;
  final double selectedBalance;
  final int spendableCount;
  final int lockedCount;
  final int selectedCount;
  final String balanceStr;
  final String outputCountStr;
  final String selectedStr;

  const BalanceInfo({
    required this.totalBalance,
    required this.unlockedBalance,
    required this.selectedBalance,
    required this.spendableCount,
    required this.lockedCount,
    required this.selectedCount,
    required this.balanceStr,
    required this.outputCountStr,
    required this.selectedStr,
  });
}

class BalanceUtils {
  /// Calculate balance information from outputs, current height, and selected outputs.
  static BalanceInfo calculate(
    List<OwnedOutput> allOutputs,
    int currentHeight,
    Set<String> selectedOutputs,
  ) {
    double totalBalance = 0;
    double unlockedBalance = 0;
    double selectedBalance = 0;
    int spendableCount = 0;
    int lockedCount = 0;
    int selectedCount = 0;
    for (var output in allOutputs) {
      if (!output.spent) {
        final amount = double.tryParse(output.amountXmr) ?? 0;
        totalBalance += amount;
        if (OutputLockUtils.isOutputUnlocked(output: output, currentHeight: currentHeight)) {
          unlockedBalance += amount;
          spendableCount++;
          final outputKey = '${output.txHash}:${output.outputIndex}';
          if (selectedOutputs.contains(outputKey)) {
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

    return BalanceInfo(
      totalBalance: totalBalance,
      unlockedBalance: unlockedBalance,
      selectedBalance: selectedBalance,
      spendableCount: spendableCount,
      lockedCount: lockedCount,
      selectedCount: selectedCount,
      balanceStr: balanceStr,
      outputCountStr: outputCountStr,
      selectedStr: selectedStr,
    );
  }
}
