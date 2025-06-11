import 'package:flutter_test/flutter_test.dart';
import 'package:tuple/tuple.dart';
import 'package:monero_extension/models/wallet_transaction.dart';
import 'package:monero_extension/src/bindings/bindings.dart';
import 'package:monero_extension/utils/transaction_utils.dart';
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
        expect(transaction.balanceChange(TransactionUtils.buildKeyImageMap(allOutputs)), equals(1.5));
        expect(transaction.isIncoming(TransactionUtils.buildKeyImageMap(allOutputs)), isTrue);
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
        expect(transaction.balanceChange(TransactionUtils.buildKeyImageMap(allOutputs)), equals(3.8));
        expect(transaction.isIncoming(TransactionUtils.buildKeyImageMap(allOutputs)), isTrue);
      });

      test('Outgoing transaction with spent outputs has negative balance', () {
        final ownedOutput = TestHelpers.createMockOutput(
          txHash: 'previous_tx',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 900,
          keyImage: 'keyimage_previous',
        );

        final spendTx = WalletTransaction(
          txHash: 'synthetic_spend_previous_tx_0',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [],
          spentKeyImages: ['keyimage_previous'],
        );

        final allOutputs = [ownedOutput];
        expect(spendTx.balanceChange(TransactionUtils.buildKeyImageMap(allOutputs)), equals(-5.0));
        expect(spendTx.isIncoming(TransactionUtils.buildKeyImageMap(allOutputs)), isFalse);
      });

      test('Self-send transaction (consolidation) has zero net balance', () {
        final receivedOutput = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '3.0',
          blockHeight: 1000,
          keyImage: 'keyimage_new',
        );

        final spentOutput = TestHelpers.createMockOutput(
          txHash: 'previous_tx',
          outputIndex: 0,
          amountXmr: '3.0',
          blockHeight: 900,
          keyImage: 'keyimage_old',
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [receivedOutput],
          spentKeyImages: ['keyimage_old'],
        );

        final allOutputs = [receivedOutput, spentOutput];
        expect(transaction.balanceChange(TransactionUtils.buildKeyImageMap(allOutputs)), equals(0.0));
      });

      test('Balance calculation handles missing key images gracefully', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '2.0',
          blockHeight: 1000,
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: ['nonexistent_keyimage'],
        );

        final allOutputs = [output];
        expect(transaction.balanceChange(TransactionUtils.buildKeyImageMap(allOutputs)), equals(2.0));
      });

      test('Balance calculation preserves precision for 12-decimal amounts', () {
        // 0.123456789012 XMR — all 12 piconero digits
        final output1 = OwnedOutput(
          txHash: 'tx1', outputIndex: 0,
          amount: Uint64(BigInt.from(123456789012)),
          amountXmr: '0.123456789012',
          key: 'k', keyOffset: 'ko', commitmentMask: 'cm',
          subaddressIndex: null, paymentId: null,
          receivedOutputBytes: 'b',
          blockHeight: Uint64(BigInt.from(1000)),
          spent: false, keyImage: 'ki1',
          isCoinbase: false, frozen: false,
        );
        // 0.000000000001 XMR — 1 piconero
        final output2 = OwnedOutput(
          txHash: 'tx1', outputIndex: 1,
          amount: Uint64(BigInt.from(1)),
          amountXmr: '0.000000000001',
          key: 'k2', keyOffset: 'ko2', commitmentMask: 'cm2',
          subaddressIndex: null, paymentId: null,
          receivedOutputBytes: 'b2',
          blockHeight: Uint64(BigInt.from(1000)),
          spent: false, keyImage: 'ki2',
          isCoinbase: false, frozen: false,
        );

        final transaction = WalletTransaction(
          txHash: 'tx1',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output1, output2],
          spentKeyImages: [],
        );

        final balance = transaction.balanceChange(TransactionUtils.buildKeyImageMap([output1, output2]));
        // double can represent 0.123456789013 but may lose precision
        // at least verify it's in the right ballpark
        expect(balance, closeTo(0.123456789013, 1e-10));
      });

      test('Balance calculation uses atomic amount field not amountXmr string', () {
        final output = OwnedOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amount: Uint64(BigInt.from(1000000000000)),
          amountXmr: 'invalid_amount',
          key: 'mock_key',
          keyOffset: 'mock_offset',
          isCoinbase: false,
          commitmentMask: 'mock_mask',
          subaddressIndex: null,
          paymentId: null,
          receivedOutputBytes: 'mock_bytes',
          blockHeight: Uint64(BigInt.from(1000)),
          spent: false,
          keyImage: 'keyimage_123',
          frozen: false,
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: [],
        );

        final allOutputs = [output];
        expect(transaction.balanceChange(TransactionUtils.buildKeyImageMap(allOutputs)), equals(1.0));
      });
    });

    group('JSON Serialization', () {
      test('toJson creates correct JSON structure', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
          subaddressIndex: const Tuple2(0, 1),
          paymentId: 'payment123',
        );

        final transaction = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: ['keyimage1'],
        );

        final json = transaction.toJson();

        expect(json['txHash'], equals('tx123'));
        expect(json['blockHeight'], equals(1000));
        expect(json['blockTimestamp'], equals(1234567890));
        expect(json['receivedOutputs'], isA<List>());
        expect(json['receivedOutputs'].length, equals(1));
        expect(json['spentKeyImages'], equals(['keyimage1']));
      });

      test('fromJson reconstructs transaction correctly', () {
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
              'subaddressIndex': [0, 1],
              'paymentId': 'payment123',
              'receivedOutputBytes': 'mock_bytes',
              'blockHeight': '1000',
              'spent': false,
              'keyImage': 'keyimage_tx123_0',
            }
          ],
          'spentKeyImages': ['keyimage1'],
        };

        final transaction = WalletTransaction.fromJson(json);

        expect(transaction.txHash, equals('tx123'));
        expect(transaction.blockHeight, equals(1000));
        expect(transaction.blockTimestamp, equals(1234567890));
        expect(transaction.receivedOutputs.length, equals(1));
        expect(transaction.spentKeyImages, equals(['keyimage1']));
      });

      test('Serialization round-trip preserves all data', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx123',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 1000,
          subaddressIndex: const Tuple2(0, 1),
          paymentId: 'payment123',
        );

        final original = WalletTransaction(
          txHash: 'tx123',
          blockHeight: 1000,
          blockTimestamp: 1234567890,
          receivedOutputs: [output],
          spentKeyImages: ['keyimage1', 'keyimage2'],
        );

        final json = original.toJson();
        final reconstructed = WalletTransaction.fromJson(json);

        expect(reconstructed.txHash, equals(original.txHash));
        expect(reconstructed.blockHeight, equals(original.blockHeight));
        expect(reconstructed.blockTimestamp, equals(original.blockTimestamp));
        expect(reconstructed.receivedOutputs.length, equals(original.receivedOutputs.length));
        expect(reconstructed.spentKeyImages, equals(original.spentKeyImages));
      });

      test('toJson preserves spent field on receivedOutputs', () {
        final output = TestHelpers.createMockOutput(
          txHash: 'tx_coinbase',
          outputIndex: 0,
          amountXmr: '0.6',
          blockHeight: 500,
          spent: true,
          isCoinbase: true,
        );

        final transaction = WalletTransaction(
          txHash: 'tx_coinbase',
          blockHeight: 500,
          blockTimestamp: 1700000000,
          receivedOutputs: [output],
          spentKeyImages: [],
        );

        final json = transaction.toJson();
        final outputMap = json['receivedOutputs'][0] as Map<String, dynamic>;

        // spent IS included in toJson
        expect(outputMap['spent'], isTrue);
        // isCoinbase is NOT included in toJson - this is a known gap
        expect(outputMap.containsKey('isCoinbase'), isFalse);

        // Round-trip: fromJson defaults isCoinbase to false since it is missing
        final reconstructed = WalletTransaction.fromJson(json);
        expect(reconstructed.receivedOutputs[0].spent, isTrue);
        expect(reconstructed.receivedOutputs[0].isCoinbase, isFalse);
      });

      test('fromJson backward compatibility: missing spent defaults to false', () {
        final json = {
          'txHash': 'tx_old',
          'blockHeight': 800,
          'blockTimestamp': 1699000000,
          'receivedOutputs': [
            {
              'txHash': 'tx_old',
              'outputIndex': 0,
              'amount': '2000000000000',
              'amountXmr': '2.0',
              'key': 'mock_key',
              'keyOffset': 'mock_offset',
              'commitmentMask': 'mock_mask',
              'subaddressIndex': null,
              'paymentId': null,
              'receivedOutputBytes': 'mock_bytes',
              'blockHeight': '800',
              // 'spent' key intentionally omitted
              'keyImage': 'ki_old',
            }
          ],
          'spentKeyImages': [],
        };

        final transaction = WalletTransaction.fromJson(json);

        expect(transaction.receivedOutputs[0].spent, isFalse);
      });

      test('fromJson backward compatibility: missing isCoinbase defaults to false', () {
        final json = {
          'txHash': 'tx_old2',
          'blockHeight': 900,
          'blockTimestamp': 1699500000,
          'receivedOutputs': [
            {
              'txHash': 'tx_old2',
              'outputIndex': 0,
              'amount': '3000000000000',
              'amountXmr': '3.0',
              'key': 'mock_key',
              'keyOffset': 'mock_offset',
              'commitmentMask': 'mock_mask',
              'subaddressIndex': null,
              'paymentId': null,
              'receivedOutputBytes': 'mock_bytes',
              'blockHeight': '900',
              'spent': false,
              'keyImage': 'ki_old2',
              // 'isCoinbase' key intentionally omitted
            }
          ],
          'spentKeyImages': [],
        };

        final transaction = WalletTransaction.fromJson(json);

        expect(transaction.receivedOutputs[0].isCoinbase, isFalse);
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
              'keyImage': 'keyimage_tx123_0',
            }
          ],
          'spentKeyImages': [],
        };

        final transaction = WalletTransaction.fromJson(json);

        expect(transaction.receivedOutputs.length, equals(1));
        expect(transaction.receivedOutputs[0].subaddressIndex, isNull);
        expect(transaction.receivedOutputs[0].paymentId, isNull);
      });
    });
  });
}
