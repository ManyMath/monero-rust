import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/src/bindings/bindings.dart';
import 'package:monero_extension/utils/output_lock_utils.dart';

OwnedOutput _makeOutput({
  int blockHeight = 100,
  bool spent = false,
  bool isCoinbase = false,
  bool frozen = false,
}) {
  return OwnedOutput(
    txHash: 'tx1',
    outputIndex: 0,
    amount: 1000000000000,
    amountXmr: '1.000000000000',
    key: 'k',
    keyOffset: 'ko',
    commitmentMask: 'cm',
    subaddressIndex: null,
    paymentId: null,
    receivedOutputBytes: '',
    blockHeight: blockHeight,
    spent: spent,
    keyImage: 'ki1',
    isCoinbase: isCoinbase,
    frozen: frozen,
  );
}

void main() {
  group('getRequiredConfirmations', () {
    test('normal output requires 10 confirmations', () {
      final output = _makeOutput(isCoinbase: false);
      expect(OutputLockUtils.getRequiredConfirmations(output), 10);
    });

    test('coinbase output requires 60 confirmations', () {
      final output = _makeOutput(isCoinbase: true);
      expect(OutputLockUtils.getRequiredConfirmations(output), 60);
    });
  });

  group('isOutputUnlocked', () {
    test('spent output returns false', () {
      final output = _makeOutput(blockHeight: 100, spent: true);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 200,
      );
      expect(result, false);
    });

    test('mempool output (height=0) returns false', () {
      final output = _makeOutput(blockHeight: 0);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 500,
      );
      expect(result, false);
    });

    test('exactly at threshold (10 confirmations) returns true', () {
      // blockHeight=100, currentHeight=110 => confirmations = 110 - 100 = 10
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 110,
      );
      expect(result, true);
    });

    test('one below threshold (9 confirmations) returns false', () {
      // blockHeight=100, currentHeight=109 => confirmations = 109 - 100 = 9
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 109,
      );
      expect(result, false);
    });

    test('well above threshold returns true', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 1000,
      );
      expect(result, true);
    });

    test('coinbase at 59 confirmations is locked', () {
      // blockHeight=100, currentHeight=159 => confirmations = 159 - 100 = 59
      final output = _makeOutput(blockHeight: 100, isCoinbase: true);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 159,
      );
      expect(result, false);
    });

    test('coinbase at 60 confirmations is unlocked', () {
      // blockHeight=100, currentHeight=160 => confirmations = 160 - 100 = 60
      final output = _makeOutput(blockHeight: 100, isCoinbase: true);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 160,
      );
      expect(result, true);
    });

    test('currentHeight == outputHeight gives 0 confirmations', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 100,
      );
      expect(result, false);
    });

    test('currentHeight < outputHeight gives 0 confirmations', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputUnlocked(
        output: output,
        currentHeight: 50,
      );
      expect(result, false);
    });
  });

  group('isOutputSpendable', () {
    test('delegates to isOutputUnlocked - unlocked case', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputSpendable(
        output: output,
        currentHeight: 200,
      );
      expect(result, true);
    });

    test('delegates to isOutputUnlocked - locked case', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.isOutputSpendable(
        output: output,
        currentHeight: 105,
      );
      expect(result, false);
    });

    test('delegates to isOutputUnlocked - spent case', () {
      final output = _makeOutput(blockHeight: 100, spent: true);
      final result = OutputLockUtils.isOutputSpendable(
        output: output,
        currentHeight: 200,
      );
      expect(result, false);
    });

    test('mempool output is not spendable', () {
      final output = _makeOutput(blockHeight: 0);
      final result = OutputLockUtils.isOutputSpendable(
        output: output,
        currentHeight: 500,
      );
      expect(result, false);
    });

    test('frozen output is not spendable even if unlocked', () {
      final output = _makeOutput(blockHeight: 100, frozen: true);
      final result = OutputLockUtils.isOutputSpendable(
        output: output,
        currentHeight: 200,
      );
      expect(result, false);
    });
  });

  group('getBlocksUntilUnlocked', () {
    test('spent output returns 0', () {
      final output = _makeOutput(blockHeight: 100, spent: true);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 102,
      );
      expect(result, 0);
    });

    test('mempool output (height=0) returns required confirmations', () {
      final output = _makeOutput(blockHeight: 0);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 500,
      );
      expect(result, 10);
    });

    test('already unlocked returns 0', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 200,
      );
      expect(result, 0);
    });

    test('needs N more blocks for normal output', () {
      // blockHeight=100, currentHeight=105 => confirmations = 5, remaining = 5
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 105,
      );
      expect(result, 5);
    });

    test('coinbase needing blocks', () {
      // blockHeight=100, currentHeight=130 => confirmations = 30, remaining = 30
      final output = _makeOutput(blockHeight: 100, isCoinbase: true);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 130,
      );
      expect(result, 30);
    });

    test('exactly at threshold returns 0', () {
      // blockHeight=100, currentHeight=110 => confirmations = 10
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 110,
      );
      expect(result, 0);
    });

    test('one below threshold returns 1', () {
      // blockHeight=100, currentHeight=109 => confirmations = 9, remaining = 1
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 109,
      );
      expect(result, 1);
    });

    test('currentHeight == outputHeight (0 confirmations) returns full required', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 100,
      );
      expect(result, 10);
    });

    test('currentHeight < outputHeight (0 confirmations) returns full required', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getBlocksUntilUnlocked(
        output: output,
        currentHeight: 50,
      );
      expect(result, 10);
    });
  });

  group('getLockStatusString', () {
    test('spent output returns "Spent"', () {
      final output = _makeOutput(blockHeight: 100, spent: true);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 200,
      );
      expect(result, 'Spent');
    });

    test('mempool output (height=0) returns "Pending (mempool)"', () {
      final output = _makeOutput(blockHeight: 0);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 500,
      );
      expect(result, 'Pending (mempool)');
    });

    test('unlocked output shows confirmations', () {
      // blockHeight=100, currentHeight=200 => confirmations = 100
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 200,
      );
      expect(result, 'Unlocked (100 confirmations)');
    });

    test('locked output shows confirmation progress', () {
      // blockHeight=100, currentHeight=105 => confirmations = 5, remaining = 5
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 105,
      );
      expect(result, 'Locked (5/10 confirmations, 5 blocks remaining)');
    });

    test('exactly at threshold shows "Unlocked"', () {
      // blockHeight=100, currentHeight=110 => confirmations = 10
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 110,
      );
      expect(result, 'Unlocked (10 confirmations)');
    });

    test('one below threshold shows "Locked"', () {
      // blockHeight=100, currentHeight=109 => confirmations = 9, remaining = 1
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 109,
      );
      expect(result, 'Locked (9/10 confirmations, 1 blocks remaining)');
    });

    test('coinbase locked shows correct required confirmations', () {
      // blockHeight=100, currentHeight=130 => confirmations = 30, remaining = 30
      final output = _makeOutput(blockHeight: 100, isCoinbase: true);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 130,
      );
      expect(result, 'Locked (30/60 confirmations, 30 blocks remaining)');
    });

    test('coinbase unlocked shows correct confirmations', () {
      // blockHeight=100, currentHeight=200 => confirmations = 100
      final output = _makeOutput(blockHeight: 100, isCoinbase: true);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 200,
      );
      expect(result, 'Unlocked (100 confirmations)');
    });

    test('currentHeight == outputHeight shows 0 confirmations locked', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 100,
      );
      expect(result, 'Locked (0/10 confirmations, 10 blocks remaining)');
    });

    test('currentHeight < outputHeight shows 0 confirmations locked', () {
      final output = _makeOutput(blockHeight: 100);
      final result = OutputLockUtils.getLockStatusString(
        output: output,
        currentHeight: 50,
      );
      expect(result, 'Locked (0/10 confirmations, 10 blocks remaining)');
    });
  });
}
