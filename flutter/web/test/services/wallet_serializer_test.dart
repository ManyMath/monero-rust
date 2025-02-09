import 'package:flutter_test/flutter_test.dart';
import '../../lib/services/wallet_serializer.dart';
import '../test_helpers.dart';

void main() {
  group('WalletSerializer - Account support', () {
    test('serialize and deserialize preserves account data', () {
      // Create mock outputs for different accounts
      final account0Outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 100,
        ),
      ];

      final account1Outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.5',
          blockHeight: 200,
        ),
      ];

      final outputsByAccount = {
        0: account0Outputs,
        1: account1Outputs,
      };

      // Serialize
      final serialized = WalletSerializer.serialize(
        seed: 'test seed phrase',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: account0Outputs, // Legacy field - account 0 outputs
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0, 1],
        outputsByAccount: outputsByAccount,
        activeAccount: 1,
      );

      // Verify serialized data contains account fields
      expect(serialized['accounts'], [0, 1]);
      expect(serialized['activeAccount'], 1);
      expect(serialized['outputsByAccount'], isNotNull);
      expect(serialized['outputsByAccount']['0'], hasLength(1));
      expect(serialized['outputsByAccount']['1'], hasLength(1));

      // Deserialize
      final deserialized = WalletSerializer.deserialize(serialized);

      // Verify account data is preserved
      expect(deserialized.accounts, [0, 1]);
      expect(deserialized.activeAccount, 1);
      expect(deserialized.outputsByAccount.keys, containsAll([0, 1]));
      expect(deserialized.outputsByAccount[0], hasLength(1));
      expect(deserialized.outputsByAccount[1], hasLength(1));

      // Verify output data integrity
      expect(deserialized.outputsByAccount[0]![0].txHash, 'tx1');
      expect(deserialized.outputsByAccount[0]![0].amountXmr, '1.5');
      expect(deserialized.outputsByAccount[1]![0].txHash, 'tx2');
      expect(deserialized.outputsByAccount[1]![0].amountXmr, '2.5');
    });

    test('deserialize handles missing account fields (backward compatibility)', () {
      // Create old-format data without account fields
      final legacyOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '1.5',
          blockHeight: 100,
        ),
      ];

      final serialized = WalletSerializer.serialize(
        seed: 'test seed phrase',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: legacyOutputs,
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        // No account fields provided
      );

      // Deserialize
      final deserialized = WalletSerializer.deserialize(serialized);

      // Verify defaults are applied
      expect(deserialized.accounts, [0]);
      expect(deserialized.activeAccount, 0);
      expect(deserialized.outputsByAccount.keys, contains(0));
      expect(deserialized.outputsByAccount[0], hasLength(1));
      expect(deserialized.outputsByAccount[0]![0].txHash, 'tx1');
    });

    test('serialize without account fields omits them from JSON', () {
      final serialized = WalletSerializer.serialize(
        seed: 'test seed phrase',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        // No account fields
      );

      // Account fields should not be present
      expect(serialized.containsKey('accounts'), false);
      expect(serialized.containsKey('activeAccount'), false);
      expect(serialized.containsKey('outputsByAccount'), false);
    });
  });
}
