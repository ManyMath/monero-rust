import 'package:flutter_test/flutter_test.dart';
import '../../lib/utils/transaction_utils.dart';
import '../../lib/models/wallet_transaction.dart';
import '../test_helpers.dart';

void main() {
  group('TransactionUtils', () {
    group('updateTransactionsFromScan', () {
      test('Creates new transaction for received outputs', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [output],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [],
          scan,
          [],
        );

        expect(result.length, equals(1));
        expect(result[0].txHash, equals('tx123'));
        expect(result[0].blockHeight, equals(1000));
        expect(result[0].receivedOutputs.length, equals(1));
        expect(result[0].spentKeyImages.length, equals(0));
      });

      test('Groups multiple outputs by transaction hash', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 1,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [output1, output2],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [],
          scan,
          [],
        );

        expect(result.length, equals(1));
        expect(result[0].txHash, equals('tx123'));
        expect(result[0].receivedOutputs.length, equals(2));
      });

      test('Creates separate transactions for different hashes', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx456',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [output1, output2],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [],
          scan,
          [],
        );

        expect(result.length, equals(2));
        expect(result.any((t) => t.txHash == 'tx123'), isTrue);
        expect(result.any((t) => t.txHash == 'tx456'), isTrue);
      });

      test('Updates existing transaction with new outputs', () {
        final existingOutput = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );

        final existing = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [existingOutput],
          spentKeyImages: [],
        );

        final newOutput = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 1,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [newOutput],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [existing],
          scan,
          [],
        );

        expect(result.length, equals(1));
        expect(result[0].txHash, equals('tx123'));
        expect(result[0].receivedOutputs.length, equals(2));
      });

      test('Prevents duplicate outputs in same transaction', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );

        final existing = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: [],
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [output], // Same output again
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [existing],
          scan,
          [],
        );

        expect(result.length, equals(1));
        expect(result[0].receivedOutputs.length, equals(1)); // Still only 1
      });

      test('Creates synthetic spend transaction for owned key images only', () {
        final ownedOutput = TestHelpers.createMockOutput(
          txHash: 'tx_old',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 900,
          keyImage: 'ki_owned',
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          spentKeyImages: ['ki_owned', 'ki_notowned'],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [],
          scan,
          [ownedOutput], // Only ki_owned belongs to us
        );

        expect(result.length, equals(1)); // Only 1 synthetic tx, not 2
        expect(result[0].txHash, equals('spend:ki_owned'));
        expect(result[0].spentKeyImages, equals(['ki_owned']));
      });

      test('Ignores key images not belonging to wallet', () {
        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          spentKeyImages: ['ki_other1', 'ki_other2', 'ki_other3'],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [],
          scan,
          [], // No owned outputs
        );

        expect(result.length, equals(0)); // No synthetic transactions
      });

      test('Updates existing synthetic spend transaction', () {
        final existing = WalletTransaction(
          txHash: 'spend:ki_123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: ['ki_123'],
        );

        final ownedOutput1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 900,
          keyImage: 'ki_123',
        );
        final ownedOutput2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 900,
          keyImage: 'ki_456',
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          spentKeyImages: ['ki_456'], // New key image
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [existing],
          scan,
          [ownedOutput1, ownedOutput2],
        );

        expect(result.length, equals(2));
        expect(result.any((t) => t.txHash == 'spend:ki_123'), isTrue);
        expect(result.any((t) => t.txHash == 'spend:ki_456'), isTrue);
      });

      test('Prevents duplicate key images in same synthetic transaction', () {
        final existing = WalletTransaction(
          txHash: 'spend:ki_123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: ['ki_123'],
        );

        final ownedOutput = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 900,
          keyImage: 'ki_123',
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          spentKeyImages: ['ki_123'], // Same key image again
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [existing],
          scan,
          [ownedOutput],
        );

        expect(result.length, equals(1));
        expect(result[0].spentKeyImages.length, equals(1)); // Still only 1
      });

      test('Handles mixed received outputs and spent key images', () {
        final receivedOutput = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '3.0',
          blockHeight: 1000,
        );
        final ownedOutput = TestHelpers.createMockOutput(
          txHash: 'tx_old',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 900,
          keyImage: 'ki_owned',
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [receivedOutput],
          spentKeyImages: ['ki_owned'],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [],
          scan,
          [ownedOutput],
        );

        expect(result.length, equals(2));
        expect(result.any((t) => t.txHash == 'tx123'), isTrue);
        expect(result.any((t) => t.txHash == 'spend:ki_owned'), isTrue);
      });

      test('Preserves existing unrelated transactions', () {
        final unrelated = WalletTransaction(
          txHash: 'tx_unrelated',
          blockHeight: 900,
          blockTimestamp: 1234560000,
          receivedOutputs: [],
          spentKeyImages: [],
        );

        final newOutput = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 1000,
        );

        final scan = TestHelpers.createMockScanResponse(
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          outputs: [newOutput],
        );

        final result = TransactionUtils.updateTransactionsFromScan(
          [unrelated],
          scan,
          [],
        );

        expect(result.length, equals(2));
        expect(result.any((t) => t.txHash == 'tx_unrelated'), isTrue);
        expect(result.any((t) => t.txHash == 'tx123'), isTrue);
      });
    });

    group('sortTransactions', () {
      test('Sorts by confirmations ascending', () {
        final tx1 = WalletTransaction(
          txHash: 'tx1',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: [],
        );
        final tx2 = WalletTransaction(
          txHash: 'tx2',
          blockHeight: 900,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: [],
        );

        final sorted = TransactionUtils.sortTransactions(
          [tx1, tx2],
          [],
          'confirms',
          true,
          1100,
        );

        expect(sorted[0].txHash, equals('tx1')); // 100 confirms
        expect(sorted[1].txHash, equals('tx2')); // 200 confirms
      });

      test('Sorts by confirmations descending', () {
        final tx1 = WalletTransaction(
          txHash: 'tx1',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: [],
        );
        final tx2 = WalletTransaction(
          txHash: 'tx2',
          blockHeight: 900,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: [],
        );

        final sorted = TransactionUtils.sortTransactions(
          [tx1, tx2],
          [],
          'confirms',
          false,
          1100,
        );

        expect(sorted[0].txHash, equals('tx2')); // 200 confirms
        expect(sorted[1].txHash, equals('tx1')); // 100 confirms
      });

      test('Sorts by amount ascending', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final tx1 = WalletTransaction(
          txHash: 'tx1',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output1],
          spentKeyImages: [],
        );
        final tx2 = WalletTransaction(
          txHash: 'tx2',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output2],
          spentKeyImages: [],
        );

        final sorted = TransactionUtils.sortTransactions(
          [tx1, tx2],
          [output1, output2],
          'amount',
          true,
          1100,
        );

        expect(sorted[0].txHash, equals('tx2')); // 2.0 XMR
        expect(sorted[1].txHash, equals('tx1')); // 5.0 XMR
      });

      test('Sorts by amount descending', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final tx1 = WalletTransaction(
          txHash: 'tx1',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output1],
          spentKeyImages: [],
        );
        final tx2 = WalletTransaction(
          txHash: 'tx2',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output2],
          spentKeyImages: [],
        );

        final sorted = TransactionUtils.sortTransactions(
          [tx1, tx2],
          [output1, output2],
          'amount',
          false,
          1100,
        );

        expect(sorted[0].txHash, equals('tx1')); // 5.0 XMR
        expect(sorted[1].txHash, equals('tx2')); // 2.0 XMR
      });

      test('Uses absolute value for amount sorting (outgoing txs)', () {
        final spentOutput = TestHelpers.createMockOutput(
          txHash: 'tx_old',
          outputIndex: 0,
          amountXmr: '10.0',
          blockHeight: 900,
          keyImage: 'ki_1',
        );

        final receivedOutput = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '3.0',
          blockHeight: 1000,
        );

        final tx1 = WalletTransaction(
          txHash: 'spend:ki_1',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: ['ki_1'],
        );
        final tx2 = WalletTransaction(
          txHash: 'tx2',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [receivedOutput],
          spentKeyImages: [],
        );

        final sorted = TransactionUtils.sortTransactions(
          [tx1, tx2],
          [spentOutput, receivedOutput],
          'amount',
          true,
          1100,
        );

        // tx2 has 3.0, tx1 has abs(-10.0) = 10.0
        expect(sorted[0].txHash, equals('tx2')); // Smaller absolute amount
        expect(sorted[1].txHash, equals('spend:ki_1')); // Larger absolute amount
      });

      test('Handles empty transaction list', () {
        final sorted = TransactionUtils.sortTransactions(
          [],
          [],
          'confirms',
          true,
          1100,
        );

        expect(sorted.length, equals(0));
      });
    });

    group('sortOutputs', () {
      test('Filters out spent outputs when showSpent is false', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 1000,
          spent: false,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
          spent: true,
        );

        final sorted = TransactionUtils.sortOutputs(
          [output1, output2],
          'confirms',
          true,
          1100,
          false, // Don't show spent
        );

        expect(sorted.length, equals(1));
        expect(sorted[0].txHash, equals('tx1'));
      });

      test('Includes spent outputs when showSpent is true', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 1000,
          spent: false,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
          spent: true,
        );

        final sorted = TransactionUtils.sortOutputs(
          [output1, output2],
          'confirms',
          true,
          1100,
          true, // Show spent
        );

        expect(sorted.length, equals(2));
      });

      test('Sorts by confirmations ascending', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 900,
        );

        final sorted = TransactionUtils.sortOutputs(
          [output1, output2],
          'confirms',
          true,
          1100,
          false,
        );

        expect(sorted[0].txHash, equals('tx1')); // 100 confirms
        expect(sorted[1].txHash, equals('tx2')); // 200 confirms
      });

      test('Sorts by confirmations descending', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.0',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 900,
        );

        final sorted = TransactionUtils.sortOutputs(
          [output1, output2],
          'confirms',
          false,
          1100,
          false,
        );

        expect(sorted[0].txHash, equals('tx2')); // 200 confirms
        expect(sorted[1].txHash, equals('tx1')); // 100 confirms
      });

      test('Sorts by value ascending', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final sorted = TransactionUtils.sortOutputs(
          [output1, output2],
          'value',
          true,
          1100,
          false,
        );

        expect(sorted[0].amountXmr, equals('2.0'));
        expect(sorted[1].amountXmr, equals('5.0'));
      });

      test('Sorts by value descending', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final sorted = TransactionUtils.sortOutputs(
          [output1, output2],
          'value',
          false,
          1100,
          false,
        );

        expect(sorted[0].amountXmr, equals('5.0'));
        expect(sorted[1].amountXmr, equals('2.0'));
      });

      test('Handles empty output list', () {
        final sorted = TransactionUtils.sortOutputs(
          [],
          'confirms',
          true,
          1100,
          false,
        );

        expect(sorted.length, equals(0));
      });
    });
  });
}
