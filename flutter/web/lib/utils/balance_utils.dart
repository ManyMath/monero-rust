import '../src/bindings/bindings.dart';
import 'output_lock_utils.dart';

class BalanceInfo {
  final double totalBalance;
  final double unlockedBalance;
  final double frozenBalance;
  final int spendableCount;
  final int lockedCount;
  final int frozenCount;
  final String balanceStr;
  final String outputCountStr;
  final String selectedStr;

  const BalanceInfo({
    required this.totalBalance,
    required this.unlockedBalance,
    required this.frozenBalance,
    required this.spendableCount,
    required this.lockedCount,
    required this.frozenCount,
    required this.balanceStr,
    required this.outputCountStr,
    required this.selectedStr,
  });
}

class BalanceUtils {
  /// Calculate balance information from outputs and current height.
  /// Frozen outputs are tracked separately.
  static BalanceInfo calculate(
    List<OwnedOutput> allOutputs,
    int currentHeight, {
    Set<String> pendingSpentKeyImages = const {},
  }) {
    int totalAtomicBalance = 0;
    int unlockedAtomicBalance = 0;
    int frozenAtomicBalance = 0;
    int spendableCount = 0;
    int lockedCount = 0;
    int frozenCount = 0;
    for (var output in allOutputs) {
      if (!output.spent && !pendingSpentKeyImages.contains(output.keyImage)) {
        final amount = output.amount.toInt();
        totalAtomicBalance += amount;
        if (output.frozen) {
          frozenAtomicBalance += amount;
          frozenCount++;
        } else if (OutputLockUtils.isOutputUnlocked(output: output, currentHeight: currentHeight)) {
          unlockedAtomicBalance += amount;
          spendableCount++;
        } else {
          lockedCount++;
        }
      }
    }
    final totalBalance = totalAtomicBalance / 1e12;
    final unlockedBalance = unlockedAtomicBalance / 1e12;
    final frozenBalance = frozenAtomicBalance / 1e12;
    final hasLockedBalance = unlockedBalance + frozenBalance < totalBalance;
    final balanceStr = hasLockedBalance
        ? '${totalBalance.toStringAsFixed(12)} XMR (Unlocked: ${unlockedBalance.toStringAsFixed(12)})'
        : '${totalBalance.toStringAsFixed(12)} XMR';
    final outputCountStr = spendableCount > 0
        ? '$spendableCount spendable output${spendableCount == 1 ? '' : 's'}'
        : lockedCount > 0
            ? '$lockedCount locked output${lockedCount == 1 ? '' : 's'}'
            : 'No outputs';
    final selectedStr = frozenCount > 0
        ? ' | Frozen: ${frozenBalance.toStringAsFixed(12)} XMR ($frozenCount)'
        : '';

    return BalanceInfo(
      totalBalance: totalBalance,
      unlockedBalance: unlockedBalance,
      frozenBalance: frozenBalance,
      spendableCount: spendableCount,
      lockedCount: lockedCount,
      frozenCount: frozenCount,
      balanceStr: balanceStr,
      outputCountStr: outputCountStr,
      selectedStr: selectedStr,
    );
  }
}
