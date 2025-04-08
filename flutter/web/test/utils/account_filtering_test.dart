import 'package:flutter_test/flutter_test.dart';
import 'package:tuple/tuple.dart';
import '../../lib/src/bindings/bindings.dart';
import '../../lib/models/wallet_transaction.dart';
import '../test_helpers.dart';

/// Filters outputs by account index, replicating the logic in DebugView._allOutputs.
/// activeAccount == -1 means "All accounts" (no filtering).
/// Outputs with null subaddressIndex are treated as belonging to account 0.
List<OwnedOutput> filterOutputsByAccount(List<OwnedOutput> allOutputs, int activeAccount) {
  if (activeAccount == -1) return allOutputs;
  return allOutputs.where((output) {
    if (output.subaddressIndex == null) return activeAccount == 0;
    return output.subaddressIndex!.item1 == activeAccount;
  }).toList();
}

/// Filters transactions by account index, replicating the logic in DebugView._allTransactions.
/// A transaction belongs to an account if:
///   - ANY receivedOutput has subaddressIndex.item1 == activeAccount (null -> account 0), OR
///   - ANY spentKeyImage maps to an output in allOutputs with subaddressIndex.item1 == activeAccount
List<WalletTransaction> filterTransactionsByAccount(
  List<WalletTransaction> allTransactions,
  List<OwnedOutput> allOutputs,
  int activeAccount,
) {
  if (activeAccount == -1) return allTransactions;
  return allTransactions.where((tx) {
    final hasReceivedOutputs = tx.receivedOutputs.any((output) {
      if (output.subaddressIndex == null) return activeAccount == 0;
      return output.subaddressIndex!.item1 == activeAccount;
    });
    final hasSpentOutputs = tx.spentKeyImages.any((keyImage) {
      final spentOutput = allOutputs.where((o) => o.keyImage == keyImage).firstOrNull;
      if (spentOutput == null) return false;
      if (spentOutput.subaddressIndex == null) return activeAccount == 0;
      return spentOutput.subaddressIndex!.item1 == activeAccount;
    });
    return hasReceivedOutputs || hasSpentOutputs;
  }).toList();
}

void main() {
  group('filterOutputsByAccount', () {
    test('All account returns all outputs', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
          subaddressIndex: const Tuple2(0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
          subaddressIndex: const Tuple2(1, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx3', outputIndex: 0, amountXmr: '3.0', blockHeight: 300,
          subaddressIndex: const Tuple2(2, 5),
        ),
      ];

      final result = filterOutputsByAccount(outputs, -1);

      expect(result.length, 3);
      expect(result, same(outputs), reason: 'Should return the same list reference for -1');
    });

    test('account 0 includes outputs with null subaddressIndex', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx_null', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
          subaddressIndex: null,
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_acct1', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
          subaddressIndex: const Tuple2(1, 0),
        ),
      ];

      final result = filterOutputsByAccount(outputs, 0);

      expect(result.length, 1);
      expect(result[0].txHash, 'tx_null');
    });

    test('account 0 includes outputs with subaddressIndex (0, x)', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx_0_0', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
          subaddressIndex: const Tuple2(0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_0_3', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
          subaddressIndex: const Tuple2(0, 3),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_1_0', outputIndex: 0, amountXmr: '3.0', blockHeight: 300,
          subaddressIndex: const Tuple2(1, 0),
        ),
      ];

      final result = filterOutputsByAccount(outputs, 0);

      expect(result.length, 2);
      expect(result.map((o) => o.txHash).toList(), ['tx_0_0', 'tx_0_3']);
    });

    test('account 1 excludes account 0 outputs', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx_null', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
          subaddressIndex: null,
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_0_0', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
          subaddressIndex: const Tuple2(0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_1_0', outputIndex: 0, amountXmr: '3.0', blockHeight: 300,
          subaddressIndex: const Tuple2(1, 0),
        ),
      ];

      final result = filterOutputsByAccount(outputs, 1);

      expect(result.length, 1);
      expect(result[0].txHash, 'tx_1_0');
    });

    test('account 1 includes only account 1 outputs', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx_0_0', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
          subaddressIndex: const Tuple2(0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_1_0', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
          subaddressIndex: const Tuple2(1, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_1_5', outputIndex: 0, amountXmr: '3.0', blockHeight: 300,
          subaddressIndex: const Tuple2(1, 5),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx_2_0', outputIndex: 0, amountXmr: '4.0', blockHeight: 400,
          subaddressIndex: const Tuple2(2, 0),
        ),
      ];

      final result = filterOutputsByAccount(outputs, 1);

      expect(result.length, 2);
      expect(result.map((o) => o.txHash).toList(), ['tx_1_0', 'tx_1_5']);
    });

    test('empty outputs returns empty', () {
      final result = filterOutputsByAccount([], 0);
      expect(result, isEmpty);

      final resultAll = filterOutputsByAccount([], -1);
      expect(resultAll, isEmpty);
    });
  });

  group('filterTransactionsByAccount', () {
    test('All account returns all transactions', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
          subaddressIndex: const Tuple2(0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
          subaddressIndex: const Tuple2(1, 0),
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [outputs[0]], spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'tx2', blockHeight: 200, blockTimestamp: 1700001000,
          receivedOutputs: [outputs[1]], spentKeyImages: [],
        ),
      ];

      final result = filterTransactionsByAccount(transactions, outputs, -1);

      expect(result.length, 2);
      expect(result, same(transactions), reason: 'Should return the same list reference for -1');
    });

    test('filters by received output account', () {
      final acct0Output = TestHelpers.createMockOutput(
        txHash: 'tx_acct0', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
        subaddressIndex: const Tuple2(0, 0),
      );
      final acct1Output = TestHelpers.createMockOutput(
        txHash: 'tx_acct1', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
        subaddressIndex: const Tuple2(1, 0),
      );
      final allOutputs = [acct0Output, acct1Output];
      final transactions = [
        WalletTransaction(
          txHash: 'tx_acct0', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [acct0Output], spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'tx_acct1', blockHeight: 200, blockTimestamp: 1700001000,
          receivedOutputs: [acct1Output], spentKeyImages: [],
        ),
      ];

      final resultAcct0 = filterTransactionsByAccount(transactions, allOutputs, 0);
      expect(resultAcct0.length, 1);
      expect(resultAcct0[0].txHash, 'tx_acct0');

      final resultAcct1 = filterTransactionsByAccount(transactions, allOutputs, 1);
      expect(resultAcct1.length, 1);
      expect(resultAcct1[0].txHash, 'tx_acct1');
    });

    test('filters by spent key image account', () {
      final acct1Output = TestHelpers.createMockOutput(
        txHash: 'old_tx', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
        keyImage: 'ki_acct1', subaddressIndex: const Tuple2(1, 0),
      );
      final allOutputs = [acct1Output];

      // Synthetic spend transaction: no received outputs, just a spent key image
      final spendTx = WalletTransaction(
        txHash: 'spend:ki_acct1', blockHeight: 200, blockTimestamp: 1700001000,
        receivedOutputs: [], spentKeyImages: ['ki_acct1'],
      );

      final resultAcct0 = filterTransactionsByAccount([spendTx], allOutputs, 0);
      expect(resultAcct0, isEmpty, reason: 'Spend from account 1 should not appear for account 0');

      final resultAcct1 = filterTransactionsByAccount([spendTx], allOutputs, 1);
      expect(resultAcct1.length, 1);
      expect(resultAcct1[0].txHash, 'spend:ki_acct1');
    });

    test('tx with null subaddressIndex on received output belongs to account 0', () {
      final nullIndexOutput = TestHelpers.createMockOutput(
        txHash: 'tx_null', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
        subaddressIndex: null,
      );
      final allOutputs = [nullIndexOutput];
      final transactions = [
        WalletTransaction(
          txHash: 'tx_null', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [nullIndexOutput], spentKeyImages: [],
        ),
      ];

      final resultAcct0 = filterTransactionsByAccount(transactions, allOutputs, 0);
      expect(resultAcct0.length, 1, reason: 'Null subaddressIndex should belong to account 0');

      final resultAcct1 = filterTransactionsByAccount(transactions, allOutputs, 1);
      expect(resultAcct1, isEmpty, reason: 'Null subaddressIndex should not belong to account 1');
    });

    test('tx with unknown spent key image excluded', () {
      // The spent key image does not correspond to any output in allOutputs
      final spendTx = WalletTransaction(
        txHash: 'spend:ki_unknown', blockHeight: 200, blockTimestamp: 1700001000,
        receivedOutputs: [], spentKeyImages: ['ki_unknown'],
      );

      final resultAcct0 = filterTransactionsByAccount([spendTx], [], 0);
      expect(resultAcct0, isEmpty,
        reason: 'Spent key image not in allOutputs should not match any account');

      final resultAcct1 = filterTransactionsByAccount([spendTx], [], 1);
      expect(resultAcct1, isEmpty);
    });

    test('tx appears for account if EITHER received or spent matches', () {
      final acct0Output = TestHelpers.createMockOutput(
        txHash: 'mixed_tx', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
        subaddressIndex: const Tuple2(0, 0),
      );
      final acct1SpentOutput = TestHelpers.createMockOutput(
        txHash: 'old_tx', outputIndex: 0, amountXmr: '5.0', blockHeight: 50,
        keyImage: 'ki_acct1', subaddressIndex: const Tuple2(1, 0),
      );
      final allOutputs = [acct0Output, acct1SpentOutput];

      // This tx receives on account 0, and spends from account 1
      final mixedTx = WalletTransaction(
        txHash: 'mixed_tx', blockHeight: 100, blockTimestamp: 1700000000,
        receivedOutputs: [acct0Output], spentKeyImages: ['ki_acct1'],
      );

      final resultAcct0 = filterTransactionsByAccount([mixedTx], allOutputs, 0);
      expect(resultAcct0.length, 1,
        reason: 'Tx with received output on account 0 should appear for account 0');

      final resultAcct1 = filterTransactionsByAccount([mixedTx], allOutputs, 1);
      expect(resultAcct1.length, 1,
        reason: 'Tx spending from account 1 should appear for account 1');
    });

    test('tx with no matching account excluded', () {
      final acct0Output = TestHelpers.createMockOutput(
        txHash: 'tx_acct0', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
        subaddressIndex: const Tuple2(0, 0),
      );
      final allOutputs = [acct0Output];
      final transactions = [
        WalletTransaction(
          txHash: 'tx_acct0', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [acct0Output], spentKeyImages: [],
        ),
      ];

      final resultAcct2 = filterTransactionsByAccount(transactions, allOutputs, 2);
      expect(resultAcct2, isEmpty,
        reason: 'Tx with only account 0 outputs should not appear for account 2');
    });
  });
}
