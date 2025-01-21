import 'package:flutter_test/flutter_test.dart';
import 'package:tuple/tuple.dart';
import '../../lib/utils/transaction_utils.dart';
import '../../lib/models/wallet_transaction.dart';
import '../../lib/src/bindings/bindings.dart';
import '../test_helpers.dart';

void main() {
  group('TransactionUtils.updateTransactionsFromScan', () {
    test('adds new transaction from scan with single output', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'abc123',
        outputIndex: 0,
        amountXmr: '1.0',
        blockHeight: 500,
      );
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500,
        blockTimestamp: 1700000000,
        outputs: [output],
      );

      final result = TransactionUtils.updateTransactionsFromScan([], scan, []);

      expect(result.length, 1);
      expect(result[0].txHash, 'abc123');
      expect(result[0].blockHeight, 500);
      expect(result[0].blockTimestamp, 1700000000);
      expect(result[0].receivedOutputs.length, 1);
      expect(result[0].spentKeyImages, isEmpty);
    });

    test('groups multiple outputs from same tx into one transaction', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 1, amountXmr: '0.5', blockHeight: 500,
      );
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000, outputs: [out1, out2],
      );

      final result = TransactionUtils.updateTransactionsFromScan([], scan, []);

      expect(result.length, 1);
      expect(result[0].receivedOutputs.length, 2);
    });

    test('creates separate transactions for different tx hashes', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 500,
      );
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000, outputs: [out1, out2],
      );

      final result = TransactionUtils.updateTransactionsFromScan([], scan, []);

      expect(result.length, 2);
      final txHashes = result.map((t) => t.txHash).toSet();
      expect(txHashes, containsAll(['tx1', 'tx2']));
    });

    test('preserves existing transactions when scan has no new ones', () {
      final existing = [
        WalletTransaction(
          txHash: 'old_tx', blockHeight: 400, blockTimestamp: 1699000000,
          receivedOutputs: [TestHelpers.createMockOutput(
            txHash: 'old_tx', outputIndex: 0, amountXmr: '3.0', blockHeight: 400,
          )],
          spentKeyImages: [],
        ),
      ];
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000, outputs: [],
      );

      final result = TransactionUtils.updateTransactionsFromScan(existing, scan, []);

      expect(result.length, 1);
      expect(result[0].txHash, 'old_tx');
    });

    test('does not duplicate outputs when same output appears again', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final existing = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 500, blockTimestamp: 1700000000,
          receivedOutputs: [output], spentKeyImages: [],
        ),
      ];
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000, outputs: [output],
      );

      final result = TransactionUtils.updateTransactionsFromScan(existing, scan, []);

      expect(result.length, 1);
      expect(result[0].receivedOutputs.length, 1);
    });

    test('adds new output to existing transaction', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 1, amountXmr: '0.5', blockHeight: 500,
      );
      final existing = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 500, blockTimestamp: 1700000000,
          receivedOutputs: [out1], spentKeyImages: [],
        ),
      ];
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000, outputs: [out2],
      );

      final result = TransactionUtils.updateTransactionsFromScan(existing, scan, []);

      expect(result.length, 1);
      expect(result[0].receivedOutputs.length, 2);
    });

    test('creates synthetic spend transaction for spent key images', () {
      final spentOutput = TestHelpers.createMockOutput(
        txHash: 'old_tx', outputIndex: 0, amountXmr: '5.0',
        blockHeight: 400, keyImage: 'ki_spent',
      );
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000,
        outputs: [], spentKeyImages: ['ki_spent'],
      );

      final result = TransactionUtils.updateTransactionsFromScan(
        [], scan, [spentOutput],
      );

      expect(result.length, 1);
      expect(result[0].txHash, 'spend:ki_spent');
      expect(result[0].receivedOutputs, isEmpty);
      expect(result[0].spentKeyImages, ['ki_spent']);
    });

    test('ignores spent key image if no matching output in allOutputs', () {
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000,
        outputs: [], spentKeyImages: ['unknown_ki'],
      );

      final result = TransactionUtils.updateTransactionsFromScan([], scan, []);

      expect(result, isEmpty);
    });

    test('does not duplicate synthetic spend if already tracked', () {
      final spentOutput = TestHelpers.createMockOutput(
        txHash: 'old_tx', outputIndex: 0, amountXmr: '5.0',
        blockHeight: 400, keyImage: 'ki_spent',
      );
      final existing = [
        WalletTransaction(
          txHash: 'spend:ki_spent', blockHeight: 450, blockTimestamp: 1699500000,
          receivedOutputs: [], spentKeyImages: ['ki_spent'],
        ),
      ];
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000,
        outputs: [], spentKeyImages: ['ki_spent'],
      );

      final result = TransactionUtils.updateTransactionsFromScan(
        existing, scan, [spentOutput],
      );

      expect(result.length, 1);
      expect(result[0].txHash, 'spend:ki_spent');
    });

    test('handles scan with both new outputs and spent key images', () {
      final newOutput = TestHelpers.createMockOutput(
        txHash: 'new_tx', outputIndex: 0, amountXmr: '2.0', blockHeight: 500,
      );
      final spentOutput = TestHelpers.createMockOutput(
        txHash: 'old_tx', outputIndex: 0, amountXmr: '3.0',
        blockHeight: 400, keyImage: 'ki_old',
      );
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000,
        outputs: [newOutput], spentKeyImages: ['ki_old'],
      );

      final result = TransactionUtils.updateTransactionsFromScan(
        [], scan, [spentOutput],
      );

      expect(result.length, 2);
      final txHashes = result.map((t) => t.txHash).toSet();
      expect(txHashes, containsAll(['new_tx', 'spend:ki_old']));
    });

    test('does not modify the original existing transactions list', () {
      final existing = <WalletTransaction>[];
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 500, blockTimestamp: 1700000000, outputs: [output],
      );

      final result = TransactionUtils.updateTransactionsFromScan(existing, scan, []);

      expect(existing, isEmpty);
      expect(result.length, 1);
    });

    test('preserves existing non-zero blockHeight when merging outputs', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 1, amountXmr: '0.5', blockHeight: 500,
      );
      final existing = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 450, blockTimestamp: 1699000000,
          receivedOutputs: [out1], spentKeyImages: [],
        ),
      ];
      // Scan at a different height sees the same tx
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 600, blockTimestamp: 1700000000, outputs: [out2],
      );

      final result = TransactionUtils.updateTransactionsFromScan(existing, scan, []);

      expect(result.length, 1);
      // Should keep existing height 450, not overwrite with scan's 600
      expect(result[0].blockHeight, 450);
      expect(result[0].blockTimestamp, 1699000000);
    });

    test('uses scan blockHeight when existing has zero height', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 500,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 1, amountXmr: '0.5', blockHeight: 500,
      );
      final existing = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 0, blockTimestamp: 0,
          receivedOutputs: [out1], spentKeyImages: [],
        ),
      ];
      final scan = TestHelpers.createMockScanResponse(
        blockHeight: 600, blockTimestamp: 1700000000, outputs: [out2],
      );

      final result = TransactionUtils.updateTransactionsFromScan(existing, scan, []);

      expect(result.length, 1);
      // Should use scan's height since existing is 0
      expect(result[0].blockHeight, 600);
      expect(result[0].blockTimestamp, 1700000000);
    });
  });

  group('TransactionUtils.sortTransactions', () {
    test('sorts by confirmations descending by default', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
      );
      final transactions = [
        WalletTransaction(txHash: 'tx1', blockHeight: 100, blockTimestamp: 0,
          receivedOutputs: [out1], spentKeyImages: []),
        WalletTransaction(txHash: 'tx2', blockHeight: 200, blockTimestamp: 0,
          receivedOutputs: [out2], spentKeyImages: []),
      ];

      final sorted = TransactionUtils.sortTransactions(
        transactions, [out1, out2], 'confirms', false, 300,
      );

      // tx1 at height 100 has 200 confirms, tx2 at 200 has 100 confirms
      // Descending: tx1 first (more confirms)
      expect(sorted[0].txHash, 'tx1');
      expect(sorted[1].txHash, 'tx2');
    });

    test('sorts by amount', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
      );
      final transactions = [
        WalletTransaction(txHash: 'tx1', blockHeight: 100, blockTimestamp: 0,
          receivedOutputs: [out1], spentKeyImages: []),
        WalletTransaction(txHash: 'tx2', blockHeight: 100, blockTimestamp: 0,
          receivedOutputs: [out2], spentKeyImages: []),
      ];

      final sorted = TransactionUtils.sortTransactions(
        transactions, [out1, out2], 'amount', false, 300,
      );

      expect(sorted[0].txHash, 'tx2');
      expect(sorted[1].txHash, 'tx1');
    });

    test('sorts by confirmations ascending', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
      );
      final transactions = [
        WalletTransaction(txHash: 'tx1', blockHeight: 100, blockTimestamp: 0,
          receivedOutputs: [out1], spentKeyImages: []),
        WalletTransaction(txHash: 'tx2', blockHeight: 200, blockTimestamp: 0,
          receivedOutputs: [out2], spentKeyImages: []),
      ];

      final sorted = TransactionUtils.sortTransactions(
        transactions, [out1, out2], 'confirms', true, 300,
      );

      // Ascending: tx2 first (fewer confirms)
      expect(sorted[0].txHash, 'tx2');
      expect(sorted[1].txHash, 'tx1');
    });

    test('sorts by amount ascending', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
      );
      final transactions = [
        WalletTransaction(txHash: 'tx1', blockHeight: 100, blockTimestamp: 0,
          receivedOutputs: [out1], spentKeyImages: []),
        WalletTransaction(txHash: 'tx2', blockHeight: 100, blockTimestamp: 0,
          receivedOutputs: [out2], spentKeyImages: []),
      ];

      final sorted = TransactionUtils.sortTransactions(
        transactions, [out1, out2], 'amount', true, 300,
      );

      // Ascending: smallest first
      expect(sorted[0].txHash, 'tx1');
      expect(sorted[1].txHash, 'tx2');
    });
  });

  group('TransactionUtils.sortOutputs', () {
    test('filters out spent outputs when showSpent is false', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 100,
        spent: true,
      );

      final sorted = TransactionUtils.sortOutputs(
        [out1, out2], 'amount', true, 300, false,
      );

      expect(sorted.length, 1);
      expect(sorted[0].txHash, 'tx1');
    });

    test('includes spent outputs when showSpent is true', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 100,
        spent: true,
      );

      final sorted = TransactionUtils.sortOutputs(
        [out1, out2], 'amount', true, 300, true,
      );

      expect(sorted.length, 2);
    });

    test('sorts by amount ascending', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '3.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out3 = TestHelpers.createMockOutput(
        txHash: 'tx3', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
      );

      final sorted = TransactionUtils.sortOutputs(
        [out1, out2, out3], 'amount', true, 300, true,
      );

      expect(sorted[0].txHash, 'tx2');
      expect(sorted[1].txHash, 'tx1');
      expect(sorted[2].txHash, 'tx3');
    });

    test('sorts by amount descending', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '3.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );

      final sorted = TransactionUtils.sortOutputs(
        [out1, out2], 'amount', false, 300, true,
      );

      expect(sorted[0].txHash, 'tx1');
      expect(sorted[1].txHash, 'tx2');
    });

    test('sorts by confirmations descending', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '1.0', blockHeight: 250,
      );

      final sorted = TransactionUtils.sortOutputs(
        [out1, out2], 'confirms', false, 300, true,
      );

      // tx1 at 100 has 200 confirms, tx2 at 250 has 50 confirms
      // Descending: tx1 first
      expect(sorted[0].txHash, 'tx1');
      expect(sorted[1].txHash, 'tx2');
    });

    test('sorts by confirmations ascending', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '1.0', blockHeight: 250,
      );

      final sorted = TransactionUtils.sortOutputs(
        [out1, out2], 'confirms', true, 300, true,
      );

      // Ascending: tx2 first (fewer confirms)
      expect(sorted[0].txHash, 'tx2');
      expect(sorted[1].txHash, 'tx1');
    });
  });
}
