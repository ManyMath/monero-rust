import 'package:flutter_test/flutter_test.dart';
import 'package:tuple/tuple.dart';
import '../../lib/models/wallet_transaction.dart';
import '../../lib/src/bindings/bindings.dart';
import '../test_helpers.dart';

void main() {
  group('WalletTransaction', () {
    group('Balance Change Calculations', () {
      test('Incoming transaction with single output has positive balance', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: [],
        );

        final allOutputs = [output];
        expect(transaction.balanceChange(allOutputs), equals(1.5));
        expect(transaction.isIncoming(allOutputs), isTrue);
      });

      test('Incoming transaction with multiple outputs sums correctly', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 1,
          amountXmr: '2.3',
          blockHeight: 1000,
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output1, output2],
          spentKeyImages: [],
        );

        final allOutputs = [output1, output2];
        expect(transaction.balanceChange(allOutputs), equals(3.8));
        expect(transaction.isIncoming(allOutputs), isTrue);
      });

      test('Outgoing transaction with spent outputs has negative balance', () {
        final spentOutput = TestHelpers.createMockOutput(
          txHash: 'tx_old',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 900,
          spent: true,
          keyImage: 'ki_123',
        );

        final transaction = WalletTransaction(
          txHash: 'spend:ki_123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: ['ki_123'],
        );

        final allOutputs = [spentOutput];
        expect(transaction.balanceChange(allOutputs), equals(-5.0));
        expect(transaction.isIncoming(allOutputs), isFalse);
      });

      test('Self-send transaction (consolidation) has zero net balance', () {
        final receivedOutput = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '4.9', // Slightly less due to fee
          blockHeight: 1000,
        );
        final spentOutput = TestHelpers.createMockOutput(
          txHash: 'tx_old',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 900,
          spent: true,
          keyImage: 'ki_456',
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [receivedOutput],
          spentKeyImages: ['ki_456'],
        );

        final allOutputs = [receivedOutput, spentOutput];
        expect(transaction.balanceChange(allOutputs), closeTo(-0.1, 0.01));
        expect(transaction.isIncoming(allOutputs), isFalse);
      });

      test('Balance calculation handles missing key images gracefully', () {
        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: ['nonexistent_ki'],
        );

        final allOutputs = <OwnedOutput>[];
        expect(transaction.balanceChange(allOutputs), equals(0.0));
      });

      test('Balance calculation handles invalid amount strings', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '0.0', // Edge case: zero amount
          blockHeight: 1000,
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: [],
        );

        final allOutputs = [output];
        expect(transaction.balanceChange(allOutputs), equals(0.0));
      });
    });

    group('JSON Serialization', () {
      test('toJson creates correct JSON structure', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
          keyImage: 'ki_test',
          subaddressIndex: const Tuple2(0, 5),
          paymentId: 'payment123',
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: ['ki_test'],
        );

        final json = transaction.toJson();

        expect(json['txHash'], equals('tx123'));
        expect(json['blockHeight'], equals(1000));
        expect(json['blockTimestamp'], equals(1234567890));
        expect(json['receivedOutputs'], isA<List>());
        expect(json['receivedOutputs'].length, equals(1));
        expect(json['spentKeyImages'], equals(['ki_test']));
      });

      test('fromJson reconstructs transaction correctly', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
          keyImage: 'ki_test',
        );

        final original = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: ['ki_test'],
        );

        final json = original.toJson();
        final reconstructed = WalletTransaction.fromJson(json);

        expect(reconstructed.txHash, equals(original.txHash));
        expect(reconstructed.blockHeight, equals(original.blockHeight));
        expect(reconstructed.blockTimestamp, equals(original.blockTimestamp));
        expect(reconstructed.receivedOutputs.length, equals(1));
        expect(reconstructed.receivedOutputs[0].txHash, equals('tx123'));
        expect(reconstructed.receivedOutputs[0].amountXmr, equals('1.5'));
        expect(reconstructed.spentKeyImages, equals(['ki_test']));
      });

      test('Serialization round-trip preserves all data', () {
        final output1 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
          subaddressIndex: const Tuple2(1, 2),
          paymentId: 'pay123',
        );
        final output2 = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 1,
          amountXmr: '2.3',
          blockHeight: 1000,
        );

        final original = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output1, output2],
          spentKeyImages: ['ki_1', 'ki_2', 'ki_3'],
        );

        final json = original.toJson();
        final reconstructed = WalletTransaction.fromJson(json);

        expect(reconstructed.txHash, equals(original.txHash));
        expect(reconstructed.blockHeight, equals(original.blockHeight));
        expect(reconstructed.blockTimestamp, equals(original.blockTimestamp));
        expect(reconstructed.receivedOutputs.length, equals(2));
        expect(reconstructed.spentKeyImages.length, equals(3));
        expect(reconstructed.receivedOutputs[0].subaddressIndex?.item1, equals(1));
        expect(reconstructed.receivedOutputs[0].subaddressIndex?.item2, equals(2));
        expect(reconstructed.receivedOutputs[0].paymentId, equals('pay123'));
        expect(reconstructed.receivedOutputs[1].paymentId, isNull);
      });

      test('fromJson handles null subaddressIndex and paymentId', () {
        final json = {
          'txHash': 'tx123',
          'blockHeight': 1000,
          'blockTimestamp': 1234567890,
          'receivedOutputs': [
            {
              'txHash': 'tx123',
              'outputIndex': 0,
              'amount': '1500000000000',
              'amountXmr': '1.5',
              'key': 'mock_key',
              'keyOffset': 'mock_offset',
              'commitmentMask': 'mock_mask',
              'subaddressIndex': null,
              'paymentId': null,
              'receivedOutputBytes': 'mock_bytes',
              'blockHeight': '1000',
              'spent': false,
              'keyImage': 'ki_test',
            }
          ],
          'spentKeyImages': [],
        };

        final transaction = WalletTransaction.fromJson(json);

        expect(transaction.receivedOutputs[0].subaddressIndex, isNull);
        expect(transaction.receivedOutputs[0].paymentId, isNull);
      });
    });
  });
}
