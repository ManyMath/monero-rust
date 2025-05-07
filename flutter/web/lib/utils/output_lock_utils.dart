import '../src/bindings/bindings.dart';

// These constants must stay in sync with rust/monero-rust/src/wallet_state.rs
// (CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE and CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW).
class OutputLockUtils {
  static const int normalOutputLockBlocks = 10;
  static const int coinbaseOutputLockBlocks = 60;

  static int getRequiredConfirmations(OwnedOutput output) {
    return output.isCoinbase ? coinbaseOutputLockBlocks : normalOutputLockBlocks;
  }

  static bool isOutputUnlocked({
    required OwnedOutput output,
    required int currentHeight,
  }) {
    if (output.spent) {
      return false;
    }

    final outputHeight = output.blockHeight.toInt();

    if (outputHeight == 0) {
      return true;
    }

    final confirmations = currentHeight > outputHeight
        ? currentHeight - outputHeight + 1
        : 0;

    final requiredConfirmations = getRequiredConfirmations(output);
    return confirmations >= requiredConfirmations;
  }

  static bool isOutputSpendable({
    required OwnedOutput output,
    required int currentHeight,
  }) {
    return isOutputUnlocked(output: output, currentHeight: currentHeight);
  }

  static int getBlocksUntilUnlocked({
    required OwnedOutput output,
    required int currentHeight,
  }) {
    if (output.spent) {
      return 0;
    }

    final outputHeight = output.blockHeight.toInt();

    if (outputHeight == 0) {
      return 0;
    }

    final confirmations = currentHeight > outputHeight
        ? currentHeight - outputHeight + 1
        : 0;

    final requiredConfirmations = getRequiredConfirmations(output);

    if (confirmations >= requiredConfirmations) {
      return 0;
    }

    return requiredConfirmations - confirmations;
  }

  static String getLockStatusString({
    required OwnedOutput output,
    required int currentHeight,
  }) {
    if (output.spent) {
      return 'Spent';
    }

    final outputHeight = output.blockHeight.toInt();

    if (outputHeight == 0) {
      return 'Pending (mempool)';
    }

    final confirmations = currentHeight > outputHeight
        ? currentHeight - outputHeight + 1
        : 0;

    final requiredConfirmations = getRequiredConfirmations(output);

    if (confirmations >= requiredConfirmations) {
      return 'Unlocked ($confirmations confirmations)';
    }

    final remaining = requiredConfirmations - confirmations;
    return 'Locked ($confirmations/$requiredConfirmations confirmations, $remaining blocks remaining)';
  }
}