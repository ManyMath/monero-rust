import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/transaction_service.dart';
import 'package:monero_extension/src/bindings/bindings.dart';
import '../test_helpers.dart';

const _validSeed =
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon abandon abandon abandon abandon abandon abandon';

const _nodeUrl = 'node.monero.com:18081';

/// Create a spendable output at a height far below currentHeight
/// so it passes the 10-confirmation check.
OwnedOutput _spendableOutput({
  String txHash = 'tx1',
  int outputIndex = 0,
  String amountXmr = '1.000000000000',
  int blockHeight = 100,
}) {
  return TestHelpers.createMockOutput(
    txHash: txHash,
    outputIndex: outputIndex,
    amountXmr: amountXmr,
    blockHeight: blockHeight,
  );
}

void main() {
  group('TransactionService.validateTransactionCreation', () {
    test('empty seed returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: '',
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '1.0')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('valid seed phrase'));
    });

    test('invalid seed (wrong word count) returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: 'one two three',
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '1.0')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('valid seed phrase'));
    });

    test('no outputs returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [],
        recipients: [const RecipientInput(address: 'addr', amount: '1.0')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('No outputs available'));
    });

    test('empty recipient address returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: '', amount: '1.0')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('destination address'));
      expect(result.error, contains('recipient 1'));
    });

    test('empty amount returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('amount'));
      expect(result.error, contains('recipient 1'));
    });

    test('non-numeric amount returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: 'abc')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('valid amount'));
    });

    test('negative amount returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '-5')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('valid amount'));
    });

    test('zero amount returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '0')],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('valid amount'));
    });

    test('missing node URL returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '0.5')],
        nodeUrl: '',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('whitespace-only node URL returns error', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'addr', amount: '0.5')],
        nodeUrl: '   ',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('valid single recipient returns success with correct atomic conversion', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [const RecipientInput(address: 'dest_addr', amount: '0.5')],
        nodeUrl: _nodeUrl,
        currentHeight: 200,
      );
      expect(result.isValid, true);
      expect(result.error, isNull);
      expect(result.normalizedSeed, isNotNull);
      expect(result.recipients, hasLength(1));
      expect(result.recipients![0].address, 'dest_addr');
      // 0.5 XMR = 500000000000 atomic units
      expect(result.recipients![0].amount.toInt(), 500000000000);
      // Node URL should be normalized with http://
      expect(result.nodeUrl, 'http://$_nodeUrl');
    });

    test('multiple recipients returns success', () {
      final outputs = [
        _spendableOutput(txHash: 'tx1', amountXmr: '5.000000000000'),
        _spendableOutput(txHash: 'tx2', outputIndex: 1, amountXmr: '3.000000000000'),
      ];
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: outputs,
        recipients: [
          const RecipientInput(address: 'addr1', amount: '1.0'),
          const RecipientInput(address: 'addr2', amount: '2.0'),
        ],
        nodeUrl: _nodeUrl,
        currentHeight: 200,
      );
      expect(result.isValid, true);
      expect(result.recipients, hasLength(2));
      expect(result.recipients![0].address, 'addr1');
      expect(result.recipients![0].amount.toInt(), 1000000000000);
      expect(result.recipients![1].address, 'addr2');
      expect(result.recipients![1].amount.toInt(), 2000000000000);
    });

    test('selected outputs insufficient for total returns error', () {
      // Output has 1 XMR but we try to send 2 XMR
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '1.000000000000',
        blockHeight: 100,
      );
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [output],
        recipients: [const RecipientInput(address: 'addr', amount: '2.0')],
        nodeUrl: _nodeUrl,
        selectedOutputs: {'tx1:0'},
        currentHeight: 200,
      );
      expect(result.isValid, false);
      expect(result.error, contains('insufficient'));
    });

    test('null selectedOutputs (no coin selection) returns success even if outputs are small', () {
      // With null selectedOutputs, coin selection validation is skipped
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '0.001000000000',
        blockHeight: 100,
      );
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [output],
        recipients: [const RecipientInput(address: 'addr', amount: '100.0')],
        nodeUrl: _nodeUrl,
        selectedOutputs: null,
        currentHeight: 200,
      );
      // No coin selection check when selectedOutputs is null
      expect(result.isValid, true);
    });

    test('empty selectedOutputs set skips coin selection check', () {
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '0.001000000000',
        blockHeight: 100,
      );
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [output],
        recipients: [const RecipientInput(address: 'addr', amount: '100.0')],
        nodeUrl: _nodeUrl,
        selectedOutputs: {},
        currentHeight: 200,
      );
      // Empty set means no coin selection check (code checks isNotEmpty)
      expect(result.isValid, true);
    });

    test('selected outputs sufficient returns success', () {
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '5.000000000000',
        blockHeight: 100,
      );
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [output],
        recipients: [const RecipientInput(address: 'addr', amount: '3.0')],
        nodeUrl: _nodeUrl,
        selectedOutputs: {'tx1:0'},
        currentHeight: 200,
      );
      expect(result.isValid, true);
      expect(result.selectedOutputs, ['tx1:0']);
    });

    test('second recipient with empty address returns error for recipient 2', () {
      final result = TransactionService.validateTransactionCreation(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        recipients: [
          const RecipientInput(address: 'addr1', amount: '0.1'),
          const RecipientInput(address: '', amount: '0.2'),
        ],
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('recipient 2'));
    });
  });

  group('TransactionService.validateSweepAll', () {
    test('empty seed returns error', () {
      final result = TransactionService.validateSweepAll(
        seed: '',
        availableOutputs: [_spendableOutput()],
        destinationAddress: 'dest',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('valid seed phrase'));
    });

    test('no outputs returns error', () {
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: [],
        destinationAddress: 'dest',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('No outputs available'));
    });

    test('empty destination address returns error', () {
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        destinationAddress: '',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('destination address'));
    });

    test('empty node URL returns error', () {
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: [_spendableOutput()],
        destinationAddress: 'dest',
        nodeUrl: '',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('no spendable outputs (too few confirmations) returns error', () {
      // Block height 195 with currentHeight 200 => 6 confirmations (200-195+1=6),
      // which is < 10 required for non-coinbase outputs.
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '1.000000000000',
        blockHeight: 195,
      );
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: [output],
        destinationAddress: 'dest',
        nodeUrl: _nodeUrl,
        currentHeight: 200,
      );
      expect(result.isValid, false);
      expect(result.error, contains('No spendable outputs'));
      expect(result.error, contains('10 confirmations'));
    });

    test('valid sweep returns success with correct totalAmount', () {
      final outputs = [
        _spendableOutput(
          txHash: 'tx1',
          amountXmr: '2.000000000000',
          blockHeight: 100,
        ),
        _spendableOutput(
          txHash: 'tx2',
          outputIndex: 1,
          amountXmr: '3.000000000000',
          blockHeight: 100,
        ),
      ];
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: outputs,
        destinationAddress: 'dest_addr',
        nodeUrl: _nodeUrl,
        currentHeight: 200,
      );
      expect(result.isValid, true);
      expect(result.error, isNull);
      expect(result.totalAmount, 5000000000000); // 2 + 3 = 5 XMR in atomic
      expect(result.destinationAddress, 'dest_addr');
      expect(result.nodeUrl, 'http://$_nodeUrl');
    });

    test('sweep with selected outputs uses only selected', () {
      final outputs = [
        _spendableOutput(txHash: 'tx1', amountXmr: '2.000000000000', blockHeight: 100),
        _spendableOutput(txHash: 'tx2', outputIndex: 1, amountXmr: '3.000000000000', blockHeight: 100),
      ];
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: outputs,
        destinationAddress: 'dest',
        nodeUrl: _nodeUrl,
        selectedOutputs: {'tx1:0'}, // Only select the 2 XMR output
        currentHeight: 200,
      );
      expect(result.isValid, true);
      expect(result.totalAmount, 2000000000000); // Only the selected 2 XMR
    });

    test('spent output is excluded from sweep total', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1',
        outputIndex: 0,
        amountXmr: '1.000000000000',
        blockHeight: 100,
        spent: true,
      );
      final result = TransactionService.validateSweepAll(
        seed: _validSeed,
        availableOutputs: [output],
        destinationAddress: 'dest',
        nodeUrl: _nodeUrl,
        currentHeight: 200,
      );
      expect(result.isValid, false);
      expect(result.error, contains('No spendable outputs'));
    });
  });

  group('TransactionService.validateTransactionBroadcast', () {
    test('null txResult returns error', () {
      final result = TransactionService.validateTransactionBroadcast(
        txResult: null,
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('No transaction'));
    });

    test('txResult with null txBlob returns error', () {
      final txResult = TransactionCreatedResponse(
        success: true,
        txId: 'txid',
        fee: 20000000,
        txBlob: null,
        spentOutputHashes: [],
        txKeyAdditional: [],
        changeOutputs: [],
      );
      final result = TransactionService.validateTransactionBroadcast(
        txResult: txResult,
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('No transaction'));
    });

    test('empty nodeUrl returns error', () {
      final txResult = TransactionCreatedResponse(
        success: true,
        txId: 'txid',
        fee: 20000000,
        txBlob: 'blob_data',
        spentOutputHashes: ['hash1'],
        txKeyAdditional: [],
        changeOutputs: [],
      );
      final result = TransactionService.validateTransactionBroadcast(
        txResult: txResult,
        nodeUrl: '',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('valid params returns success', () {
      final txResult = TransactionCreatedResponse(
        success: true,
        txId: 'txid',
        fee: 20000000,
        txBlob: 'blob_data',
        spentOutputHashes: ['hash1', 'hash2'],
        txKeyAdditional: [],
        changeOutputs: [],
      );
      final result = TransactionService.validateTransactionBroadcast(
        txResult: txResult,
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, true);
      expect(result.error, isNull);
      expect(result.txBlob, 'blob_data');
      expect(result.spentOutputHashes, ['hash1', 'hash2']);
      expect(result.nodeUrl, 'http://$_nodeUrl');
    });

    test('nodeUrl with https is preserved', () {
      final txResult = TransactionCreatedResponse(
        success: true,
        txId: 'txid',
        fee: 20000000,
        txBlob: 'blob_data',
        spentOutputHashes: [],
        txKeyAdditional: [],
        changeOutputs: [],
      );
      final result = TransactionService.validateTransactionBroadcast(
        txResult: txResult,
        nodeUrl: 'https://secure.node.com',
      );
      expect(result.isValid, true);
      expect(result.nodeUrl, 'https://secure.node.com');
    });
  });

  group('TransactionService.calculateMaxSpendable', () {
    test('no outputs returns 0.0', () {
      final result = TransactionService.calculateMaxSpendable(
        availableOutputs: [],
        currentHeight: 200,
      );
      expect(result, 0.0);
    });

    test('single spendable output returns total minus fee estimate', () {
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '1.000000000000',
        blockHeight: 100,
      );
      final result = TransactionService.calculateMaxSpendable(
        availableOutputs: [output],
        currentHeight: 200,
      );
      // 1 XMR = 1_000_000_000_000 atomic
      // Fee estimate = baseFee(19_000_000) + 1 * perInput(16_000_000) = 35_000_000
      // Max spendable = (1_000_000_000_000 - 35_000_000) / 1e12
      final expectedAtomic = 1000000000000 - 35000000;
      final expected = expectedAtomic / 1e12;
      expect(result, closeTo(expected, 1e-15));
    });

    test('multiple spendable outputs sums amounts and adjusts fee per input', () {
      final outputs = [
        _spendableOutput(txHash: 'tx1', amountXmr: '1.000000000000', blockHeight: 100),
        _spendableOutput(txHash: 'tx2', outputIndex: 1, amountXmr: '2.000000000000', blockHeight: 100),
      ];
      final result = TransactionService.calculateMaxSpendable(
        availableOutputs: outputs,
        currentHeight: 200,
      );
      // Total = 3_000_000_000_000
      // Fee = 19_000_000 + 2 * 16_000_000 = 51_000_000
      // Max = (3_000_000_000_000 - 51_000_000) / 1e12
      final expectedAtomic = 3000000000000 - 51000000;
      final expected = expectedAtomic / 1e12;
      expect(result, closeTo(expected, 1e-15));
    });

    test('fee exceeds total returns 0.0', () {
      // Very tiny output: 0.000000010000 XMR = 10000 atomic
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1',
        outputIndex: 0,
        amountXmr: '0.000000010000',
        blockHeight: 100,
      );
      final result = TransactionService.calculateMaxSpendable(
        availableOutputs: [output],
        currentHeight: 200,
      );
      // Fee = 35_000_000 which far exceeds 10_000 atomic
      expect(result, 0.0);
    });

    test('unspendable outputs (too few confirmations) are excluded', () {
      // blockHeight 195 with currentHeight 200 gives 6 confirmations < 10 required
      final output = _spendableOutput(
        txHash: 'tx1',
        amountXmr: '10.000000000000',
        blockHeight: 195,
      );
      final result = TransactionService.calculateMaxSpendable(
        availableOutputs: [output],
        currentHeight: 200,
      );
      expect(result, 0.0);
    });

    test('selectedOutputs limits which outputs are considered', () {
      final outputs = [
        _spendableOutput(txHash: 'tx1', amountXmr: '5.000000000000', blockHeight: 100),
        _spendableOutput(txHash: 'tx2', outputIndex: 1, amountXmr: '3.000000000000', blockHeight: 100),
      ];
      // Only select tx2
      final result = TransactionService.calculateMaxSpendable(
        availableOutputs: outputs,
        selectedOutputs: {'tx2:1'},
        currentHeight: 200,
      );
      // Only tx2 counted: 3_000_000_000_000
      // Fee = 19_000_000 + 1 * 16_000_000 = 35_000_000
      final expectedAtomic = 3000000000000 - 35000000;
      final expected = expectedAtomic / 1e12;
      expect(result, closeTo(expected, 1e-15));
    });
  });
}
