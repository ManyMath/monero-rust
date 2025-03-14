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
        outputs: account0Outputs,
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0, 1],
        outputsByAccount: outputsByAccount,
        activeAccount: 1,
        scanningAccounts: {0, 1},
      );

      // Verify serialized data contains version and account fields
      expect(serialized['version'], 1);
      expect(serialized['accounts'], [0, 1]);
      expect(serialized['activeAccount'], 1);
      expect(serialized['scanningAccounts'], [0, 1]);
      expect(serialized['outputsByAccount'], isNotNull);
      expect(serialized['outputsByAccount']['0'], hasLength(1));
      expect(serialized['outputsByAccount']['1'], hasLength(1));

      // Deserialize
      final deserialized = WalletSerializer.deserialize(serialized);

      // Verify account data is preserved
      expect(deserialized.accounts, [0, 1]);
      expect(deserialized.activeAccount, 1);
      expect(deserialized.scanningAccounts, {0, 1});
      expect(deserialized.outputsByAccount.keys, containsAll([0, 1]));
      expect(deserialized.outputsByAccount[0], hasLength(1));
      expect(deserialized.outputsByAccount[1], hasLength(1));

      // Verify output data integrity
      expect(deserialized.outputsByAccount[0]![0].txHash, 'tx1');
      expect(deserialized.outputsByAccount[0]![0].amountXmr, '1.5');
      expect(deserialized.outputsByAccount[1]![0].txHash, 'tx2');
      expect(deserialized.outputsByAccount[1]![0].amountXmr, '2.5');
    });

    test('version checking rejects incompatible versions', () {
      final serialized = WalletSerializer.serialize(
        seed: 'test seed phrase',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0],
        outputsByAccount: {0: []},
        activeAccount: 0,
        scanningAccounts: {0},
      );

      // Test missing version
      final noVersion = Map<String, dynamic>.from(serialized);
      noVersion.remove('version');
      expect(
        () => WalletSerializer.deserialize(noVersion),
        throwsA(isA<FormatException>().having(
          (e) => e.message,
          'message',
          contains('missing version'),
        )),
      );

      // Test newer version
      final newerVersion = Map<String, dynamic>.from(serialized);
      newerVersion['version'] = 999;
      expect(
        () => WalletSerializer.deserialize(newerVersion),
        throwsA(isA<FormatException>().having(
          (e) => e.message,
          'message',
          contains('newer than supported'),
        )),
      );

      // Test older version (when we increment to version 2, this would test migration)
      final olderVersion = Map<String, dynamic>.from(serialized);
      olderVersion['version'] = 0;
      expect(
        () => WalletSerializer.deserialize(olderVersion),
        throwsA(isA<FormatException>().having(
          (e) => e.message,
          'message',
          contains('no longer supported'),
        )),
      );
    });

  });
}
