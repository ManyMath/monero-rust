import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/utils/balance_utils.dart';
import '../test_helpers.dart';

void main() {
  group('BalanceUtils.calculate', () {
    test('empty outputs returns zero balances', () {
      final result = BalanceUtils.calculate([], 1000);

      expect(result.totalBalance, 0);
      expect(result.unlockedBalance, 0);
      expect(result.frozenBalance, 0);
      expect(result.spendableCount, 0);
      expect(result.lockedCount, 0);
      expect(result.frozenCount, 0);
    });

    test('unspent unlocked output contributes to total and unlocked balance', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.500000000000',
          blockHeight: 100,
        ),
      ];

      final result = BalanceUtils.calculate(outputs, 110);

      expect(result.totalBalance, 1.5);
      expect(result.unlockedBalance, 1.5);
      expect(result.spendableCount, 1);
      expect(result.lockedCount, 0);
    });

    test('spent output is excluded from all balances', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '2.000000000000',
          blockHeight: 100,
          spent: true,
        ),
      ];

      final result = BalanceUtils.calculate(outputs, 200);

      expect(result.totalBalance, 0);
      expect(result.unlockedBalance, 0);
      expect(result.spendableCount, 0);
      expect(result.lockedCount, 0);
      expect(result.frozenCount, 0);
    });

    test('locked output (insufficient confirmations) contributes to total but not unlocked', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '3.000000000000',
          blockHeight: 100,
        ),
      ];

      final result = BalanceUtils.calculate(outputs, 105);

      expect(result.totalBalance, 3.0);
      expect(result.unlockedBalance, 0);
      expect(result.spendableCount, 0);
      expect(result.lockedCount, 1);
    });

    test('coinbase output requires 60 confirmations', () {
      final coinbaseOutput = TestHelpers.createMockOutput(
        txHash: 'cb1',
        outputIndex: 0,
        amountXmr: '0.600000000000',
        blockHeight: 100,
        isCoinbase: true,
      );

      final lockedResult = BalanceUtils.calculate([coinbaseOutput], 130);
      expect(lockedResult.totalBalance, 0.6);
      expect(lockedResult.unlockedBalance, 0);
      expect(lockedResult.lockedCount, 1);
      expect(lockedResult.spendableCount, 0);

      final unlockedResult = BalanceUtils.calculate([coinbaseOutput], 165);
      expect(unlockedResult.totalBalance, 0.6);
      expect(unlockedResult.unlockedBalance, 0.6);
      expect(unlockedResult.lockedCount, 0);
      expect(unlockedResult.spendableCount, 1);
    });

    test('frozen outputs tracked separately', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.000000000000',
          blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 1,
          amountXmr: '2.000000000000',
          blockHeight: 100,
        ),
      ];
      // Freeze the second output
      outputs[1] = outputs[1].copyWith(frozen: true);

      final result = BalanceUtils.calculate(outputs, 200);

      expect(result.totalBalance, 3.0);
      expect(result.unlockedBalance, 1.0);
      expect(result.spendableCount, 1);
      expect(result.frozenBalance, 2.0);
      expect(result.frozenCount, 1);
    });

    test('frozen locked output counted as frozen not locked', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '5.000000000000',
          blockHeight: 100,
        ),
      ];
      outputs[0] = outputs[0].copyWith(frozen: true);

      final result = BalanceUtils.calculate(outputs, 105);

      expect(result.lockedCount, 0);
      expect(result.spendableCount, 0);
      expect(result.frozenBalance, 5.0);
      expect(result.frozenCount, 1);
    });

    test('multiple outputs with mix of states', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'unlocked1',
          outputIndex: 0,
          amountXmr: '1.000000000000',
          blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'unlocked2',
          outputIndex: 0,
          amountXmr: '2.000000000000',
          blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'locked1',
          outputIndex: 0,
          amountXmr: '3.000000000000',
          blockHeight: 195,
        ),
        TestHelpers.createMockOutput(
          txHash: 'spent1',
          outputIndex: 0,
          amountXmr: '4.000000000000',
          blockHeight: 100,
          spent: true,
        ),
        TestHelpers.createMockOutput(
          txHash: 'coinbase_locked',
          outputIndex: 0,
          amountXmr: '5.000000000000',
          blockHeight: 150,
          isCoinbase: true,
        ),
      ];

      final result = BalanceUtils.calculate(outputs, 200);

      expect(result.totalBalance, 11.0);
      expect(result.unlockedBalance, 3.0);
      expect(result.spendableCount, 2);
      expect(result.lockedCount, 2);
      expect(result.frozenBalance, 0);
      expect(result.frozenCount, 0);
    });
  });

  group('BalanceInfo string formatting', () {
    test('balance string formatting - no locked', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.500000000000',
          blockHeight: 100,
        ),
      ];

      final result = BalanceUtils.calculate(outputs, 200);

      expect(result.balanceStr, '1.500000000000 XMR');
    });

    test('balance string formatting - has locked', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.000000000000',
          blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.000000000000',
          blockHeight: 195,
        ),
      ];

      final result = BalanceUtils.calculate(outputs, 200);

      expect(result.balanceStr, '3.000000000000 XMR (Unlocked: 1.000000000000)');
    });

    test('output count string formatting', () {
      final unlockedOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.000000000000',
          blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.000000000000',
          blockHeight: 100,
        ),
      ];
      final spendableResult = BalanceUtils.calculate(unlockedOutputs, 200);
      expect(spendableResult.outputCountStr, '2 spendable outputs');

      final singleOutput = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.000000000000',
          blockHeight: 100,
        ),
      ];
      final singleResult = BalanceUtils.calculate(singleOutput, 200);
      expect(singleResult.outputCountStr, '1 spendable output');

      final lockedOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.000000000000',
          blockHeight: 195,
        ),
      ];
      final lockedResult = BalanceUtils.calculate(lockedOutputs, 200);
      expect(lockedResult.outputCountStr, '1 locked output');

      final emptyResult = BalanceUtils.calculate([], 200);
      expect(emptyResult.outputCountStr, 'No outputs');
    });
  });
}
