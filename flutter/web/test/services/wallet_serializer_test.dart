import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/wallet_serializer.dart';
import 'package:monero_extension/models/wallet_transaction.dart';
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
          subaddressIndex: (0, 0),
        ),
      ];

      final account1Outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.5',
          blockHeight: 200,
          subaddressIndex: (1, 0),
        ),
      ];

      // Serialize
      final serialized = WalletSerializer.serialize(
        seed: 'test seed phrase',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [...account0Outputs, ...account1Outputs],
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0, 1],
        activeAccount: 1,
        scanningAccounts: {0, 1},
      );

      // Verify serialized data contains version and account fields
      expect(serialized['version'], 1);
      expect(serialized['accounts'], [0, 1]);
      expect(serialized['activeAccount'], 1);
      expect(serialized['scanningAccounts'], [0, 1]);

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

  group('outputsByAccount derivation', () {
    test('outputsByAccount matches grouping of flat outputs list', () {
      // Create outputs for account 0 (subaddress (0,0)) and account 1 (subaddress (1,0))
      final outputAcct0 = TestHelpers.createMockOutput(
        txHash: 'tx_a0',
        outputIndex: 0,
        amountXmr: '1.0',
        blockHeight: 100,
        subaddressIndex: (0, 0),
      );
      final outputAcct1 = TestHelpers.createMockOutput(
        txHash: 'tx_a1',
        outputIndex: 0,
        amountXmr: '2.0',
        blockHeight: 200,
        subaddressIndex: (1, 0),
      );

      final allOutputs = [outputAcct0, outputAcct1];

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: allOutputs,
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0, 1],
        activeAccount: 0,
        scanningAccounts: {0, 1},
      );

      final deserialized = WalletSerializer.deserialize(serialized);

      // Group the flat outputs list by account ($1 of subaddressIndex)
      final groupedFromFlat = <int, List<dynamic>>{};
      for (final o in deserialized.outputs) {
        final account = o.subaddressIndex?.$1 ?? 0;
        groupedFromFlat.putIfAbsent(account, () => []);
        groupedFromFlat[account]!.add(o);
      }

      // Verify outputsByAccount matches the grouping derived from flat list
      expect(deserialized.outputsByAccount.keys.toSet(), groupedFromFlat.keys.toSet());
      for (final account in groupedFromFlat.keys) {
        expect(
          deserialized.outputsByAccount[account]!.length,
          groupedFromFlat[account]!.length,
        );
        expect(
          deserialized.outputsByAccount[account]![0].txHash,
          (groupedFromFlat[account]![0] as dynamic).txHash,
        );
      }
    });

    test('outputs with null subaddressIndex go to account 0', () {
      final outputNullSubaddr = TestHelpers.createMockOutput(
        txHash: 'tx_null',
        outputIndex: 0,
        amountXmr: '3.0',
        blockHeight: 150,
        // subaddressIndex defaults to null
      );

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [outputNullSubaddr],
        transactions: [],
        continuousScanCurrentHeight: 200,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      final deserialized = WalletSerializer.deserialize(serialized);

      // The output with null subaddressIndex should be in account 0
      expect(deserialized.outputsByAccount[0], hasLength(1));
      expect(deserialized.outputsByAccount[0]![0].txHash, 'tx_null');
      expect(deserialized.outputsByAccount[0]![0].subaddressIndex, isNull);
    });

    test('round-trip with 3 accounts preserves all outputs', () {
      final output0a = TestHelpers.createMockOutput(
        txHash: 'tx_0a',
        outputIndex: 0,
        amountXmr: '1.0',
        blockHeight: 100,
        subaddressIndex: (0, 0),
      );
      final output0b = TestHelpers.createMockOutput(
        txHash: 'tx_0b',
        outputIndex: 0,
        amountXmr: '1.5',
        blockHeight: 101,
        subaddressIndex: (0, 1),
      );
      final output1 = TestHelpers.createMockOutput(
        txHash: 'tx_1a',
        outputIndex: 0,
        amountXmr: '2.0',
        blockHeight: 200,
        subaddressIndex: (1, 0),
      );
      final output2a = TestHelpers.createMockOutput(
        txHash: 'tx_2a',
        outputIndex: 0,
        amountXmr: '3.0',
        blockHeight: 300,
        subaddressIndex: (2, 0),
      );
      final output2b = TestHelpers.createMockOutput(
        txHash: 'tx_2b',
        outputIndex: 1,
        amountXmr: '3.5',
        blockHeight: 301,
        subaddressIndex: (2, 1),
      );
      final output2c = TestHelpers.createMockOutput(
        txHash: 'tx_2c',
        outputIndex: 0,
        amountXmr: '4.0',
        blockHeight: 302,
        subaddressIndex: (2, 2),
      );

      final allOutputs = [output0a, output0b, output1, output2a, output2b, output2c];

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: allOutputs,
        transactions: [],
        continuousScanCurrentHeight: 400,
        selectedOutputs: {},
        accounts: [0, 1, 2],
        activeAccount: 0,
        scanningAccounts: {0, 1, 2},
      );

      final deserialized = WalletSerializer.deserialize(serialized);

      // Verify each account has correct output count
      expect(deserialized.outputsByAccount.keys.toSet(), {0, 1, 2});
      expect(deserialized.outputsByAccount[0], hasLength(2));
      expect(deserialized.outputsByAccount[1], hasLength(1));
      expect(deserialized.outputsByAccount[2], hasLength(3));

      // Verify data integrity for each account
      expect(deserialized.outputsByAccount[0]![0].txHash, 'tx_0a');
      expect(deserialized.outputsByAccount[0]![1].txHash, 'tx_0b');
      expect(deserialized.outputsByAccount[1]![0].txHash, 'tx_1a');
      expect(deserialized.outputsByAccount[2]![0].txHash, 'tx_2a');
      expect(deserialized.outputsByAccount[2]![1].txHash, 'tx_2b');
      expect(deserialized.outputsByAccount[2]![2].txHash, 'tx_2c');

      // Verify amounts preserved
      expect(deserialized.outputsByAccount[0]![0].amountXmr, '1.0');
      expect(deserialized.outputsByAccount[2]![2].amountXmr, '4.0');

      // Verify flat outputs list also has all 6
      expect(deserialized.outputs, hasLength(6));
    });
  });

  group('transaction round-trip with receivedOutputs', () {
    test('transaction receivedOutputs preserve all output fields', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx_recv',
        outputIndex: 2,
        amountXmr: '5.123',
        blockHeight: 500,
        subaddressIndex: (0, 3),
        paymentId: 'pay123',
        spent: true,
        keyImage: 'ki_recv_output',
        isCoinbase: true,
      );

      final tx = WalletTransaction(
        txHash: 'tx_recv',
        blockHeight: 500,
        blockTimestamp: 1700000000,
        receivedOutputs: [output],
        spentKeyImages: [],
      );

      final json = tx.toJson();
      final restored = WalletTransaction.fromJson(json);

      // Verify transaction-level fields
      expect(restored.txHash, 'tx_recv');
      expect(restored.blockHeight, 500);
      expect(restored.blockTimestamp, 1700000000);
      expect(restored.receivedOutputs, hasLength(1));

      // Verify every output field
      final ro = restored.receivedOutputs[0];
      expect(ro.txHash, 'tx_recv');
      expect(ro.outputIndex, 2);
      expect(ro.amountXmr, '5.123');
      expect(ro.key, output.key);
      expect(ro.keyOffset, output.keyOffset);
      expect(ro.commitmentMask, output.commitmentMask);
      expect(ro.subaddressIndex, (0, 3));
      expect(ro.paymentId, 'pay123');
      expect(ro.receivedOutputBytes, output.receivedOutputBytes);
      expect(ro.blockHeight.toInt(), 500);
      expect(ro.spent, true);
      expect(ro.keyImage, 'ki_recv_output');
      // Note: isCoinbase is not serialized by WalletTransaction.toJson(),
      // so it defaults to false on deserialization (backward compat behavior)
      expect(ro.isCoinbase, false);
    });

    test('transaction with multiple receivedOutputs and spentKeyImages', () {
      final output1 = TestHelpers.createMockOutput(
        txHash: 'tx_multi',
        outputIndex: 0,
        amountXmr: '1.0',
        blockHeight: 600,
        subaddressIndex: (0, 0),
      );
      final output2 = TestHelpers.createMockOutput(
        txHash: 'tx_multi',
        outputIndex: 1,
        amountXmr: '2.0',
        blockHeight: 600,
        subaddressIndex: (0, 1),
      );

      final tx = WalletTransaction(
        txHash: 'tx_multi',
        blockHeight: 600,
        blockTimestamp: 1700001000,
        receivedOutputs: [output1, output2],
        spentKeyImages: ['spent_ki_1', 'spent_ki_2'],
      );

      final json = tx.toJson();
      final restored = WalletTransaction.fromJson(json);

      // Verify received outputs
      expect(restored.receivedOutputs, hasLength(2));
      expect(restored.receivedOutputs[0].txHash, 'tx_multi');
      expect(restored.receivedOutputs[0].outputIndex, 0);
      expect(restored.receivedOutputs[0].amountXmr, '1.0');
      expect(restored.receivedOutputs[1].outputIndex, 1);
      expect(restored.receivedOutputs[1].amountXmr, '2.0');

      // Verify spent key images
      expect(restored.spentKeyImages, hasLength(2));
      expect(restored.spentKeyImages, ['spent_ki_1', 'spent_ki_2']);
    });

    test('full serialize/deserialize round-trip preserves transactions', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx_full',
        outputIndex: 0,
        amountXmr: '10.0',
        blockHeight: 700,
        subaddressIndex: (0, 0),
      );

      final tx = WalletTransaction(
        txHash: 'tx_full',
        blockHeight: 700,
        blockTimestamp: 1700002000,
        receivedOutputs: [output],
        spentKeyImages: ['ki_spent_full'],
      );

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [output],
        transactions: [tx],
        continuousScanCurrentHeight: 800,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      final deserialized = WalletSerializer.deserialize(serialized);

      // Verify transaction count
      expect(deserialized.transactions, hasLength(1));

      // Verify transaction fields
      final restoredTx = deserialized.transactions[0];
      expect(restoredTx.txHash, 'tx_full');
      expect(restoredTx.blockHeight, 700);

      // Verify receivedOutputs count and data
      expect(restoredTx.receivedOutputs, hasLength(1));
      expect(restoredTx.receivedOutputs[0].txHash, 'tx_full');
      expect(restoredTx.receivedOutputs[0].amountXmr, '10.0');
      expect(restoredTx.receivedOutputs[0].blockHeight.toInt(), 700);

      // Verify spentKeyImages
      expect(restoredTx.spentKeyImages, ['ki_spent_full']);
    });
  });

  group('blockHashesJson persistence', () {
    test('serialize and deserialize preserves blockHashesJson', () {
      final blockHashes = '{"hashes":{"100":"abc123","200":"def456"},"genesis_hash":"000000"}';

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
        blockHashesJson: blockHashes,
      );

      expect(serialized['blockHashesJson'], blockHashes);

      final deserialized = WalletSerializer.deserialize(serialized);
      expect(deserialized.blockHashesJson, blockHashes);
    });

    test('serialize omits blockHashesJson when null', () {
      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 300,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      expect(serialized.containsKey('blockHashesJson'), false);

      final deserialized = WalletSerializer.deserialize(serialized);
      expect(deserialized.blockHashesJson, isNull);
    });

    test('backward compat: missing blockHashesJson returns null', () {
      final serialized = {
        'version': 1,
        'seed': 'test seed',
        'network': 'stagenet',
        'address': '5addr...',
        'nodeUrl': 'http://node:38081',
        'outputs': [],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 200},
        'selectedOutputs': [],
        'accounts': [0],
        'activeAccount': 0,
        'scanningAccounts': [0],
        // blockHashesJson intentionally absent
      };

      final deserialized = WalletSerializer.deserialize(serialized);
      expect(deserialized.blockHashesJson, isNull);
    });
  });

  group('transaction description persistence', () {
    test('serialize preserves transaction description', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx_noted',
        outputIndex: 0,
        amountXmr: '1.0',
        blockHeight: 100,
        subaddressIndex: (0, 0),
      );

      final tx = WalletTransaction(
        txHash: 'tx_noted',
        blockHeight: 100,
        blockTimestamp: 1700000000,
        receivedOutputs: [output],
        spentKeyImages: [],
        description: 'Rent payment March',
      );

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [output],
        transactions: [tx],
        continuousScanCurrentHeight: 200,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      final txMap = (serialized['transactions'] as List)[0] as Map<String, dynamic>;
      expect(txMap['description'], 'Rent payment March');
    });

    test('round-trip preserves transaction description', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx_desc',
        outputIndex: 0,
        amountXmr: '5.0',
        blockHeight: 100,
        subaddressIndex: (0, 0),
      );

      final tx = WalletTransaction(
        txHash: 'tx_desc',
        blockHeight: 100,
        blockTimestamp: 1700000000,
        receivedOutputs: [output],
        spentKeyImages: [],
        description: 'Coffee payment',
      );

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [output],
        transactions: [tx],
        continuousScanCurrentHeight: 200,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      final deserialized = WalletSerializer.deserialize(serialized);
      expect(deserialized.transactions[0].description, 'Coffee payment');
    });

    test('null description is omitted in serialization', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx_no_desc',
        outputIndex: 0,
        amountXmr: '1.0',
        blockHeight: 100,
        subaddressIndex: (0, 0),
      );

      final tx = WalletTransaction(
        txHash: 'tx_no_desc',
        blockHeight: 100,
        blockTimestamp: 1700000000,
        receivedOutputs: [output],
        spentKeyImages: [],
      );

      final serialized = WalletSerializer.serialize(
        seed: 'test seed',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: [output],
        transactions: [tx],
        continuousScanCurrentHeight: 200,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      final txMap = (serialized['transactions'] as List)[0] as Map<String, dynamic>;
      expect(txMap.containsKey('description'), false);

      final deserialized = WalletSerializer.deserialize(serialized);
      expect(deserialized.transactions[0].description, isNull);
    });
  });

  group('spent and isCoinbase backward compatibility', () {
    test('deserialize handles missing spent field', () {
      // Manually construct serialized data with outputs missing the 'spent' key
      final serialized = {
        'version': 1,
        'seed': 'test seed',
        'network': 'stagenet',
        'address': '5addr...',
        'nodeUrl': 'http://node:38081',
        'outputs': [
          {
            'txHash': 'tx_no_spent',
            'outputIndex': 0,
            'amount': '1000000000000',
            'amountXmr': '1.0',
            'key': 'mock_key',
            'keyOffset': 'mock_offset',
            'commitmentMask': 'mock_mask',
            'subaddressIndex': [0, 0],
            'paymentId': null,
            'receivedOutputBytes': 'mock_bytes',
            'blockHeight': '100',
            // 'spent' intentionally omitted
            'keyImage': 'ki_no_spent',
            'isCoinbase': false,
          },
        ],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 200},
        'selectedOutputs': [],
        'accounts': [0],
        'activeAccount': 0,
        'scanningAccounts': [0],
        'outputsByAccount': {
          '0': [
            {
              'txHash': 'tx_no_spent',
              'outputIndex': 0,
              'amount': '1000000000000',
              'amountXmr': '1.0',
              'key': 'mock_key',
              'keyOffset': 'mock_offset',
              'commitmentMask': 'mock_mask',
              'subaddressIndex': [0, 0],
              'paymentId': null,
              'receivedOutputBytes': 'mock_bytes',
              'blockHeight': '100',
              // 'spent' intentionally omitted
              'keyImage': 'ki_no_spent',
              'isCoinbase': false,
            },
          ],
        },
      };

      final deserialized = WalletSerializer.deserialize(serialized);

      // spent should default to false when missing
      expect(deserialized.outputs[0].spent, false);
      expect(deserialized.outputsByAccount[0]![0].spent, false);
    });

    test('deserialize handles missing isCoinbase field', () {
      // Manually construct serialized data with outputs missing the 'isCoinbase' key
      final serialized = {
        'version': 1,
        'seed': 'test seed',
        'network': 'stagenet',
        'address': '5addr...',
        'nodeUrl': 'http://node:38081',
        'outputs': [
          {
            'txHash': 'tx_no_coinbase',
            'outputIndex': 0,
            'amount': '2000000000000',
            'amountXmr': '2.0',
            'key': 'mock_key',
            'keyOffset': 'mock_offset',
            'commitmentMask': 'mock_mask',
            'subaddressIndex': [0, 0],
            'paymentId': null,
            'receivedOutputBytes': 'mock_bytes',
            'blockHeight': '100',
            'spent': false,
            'keyImage': 'ki_no_coinbase',
            // 'isCoinbase' intentionally omitted
          },
        ],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 200},
        'selectedOutputs': [],
        'accounts': [0],
        'activeAccount': 0,
        'scanningAccounts': [0],
        'outputsByAccount': {
          '0': [
            {
              'txHash': 'tx_no_coinbase',
              'outputIndex': 0,
              'amount': '2000000000000',
              'amountXmr': '2.0',
              'key': 'mock_key',
              'keyOffset': 'mock_offset',
              'commitmentMask': 'mock_mask',
              'subaddressIndex': [0, 0],
              'paymentId': null,
              'receivedOutputBytes': 'mock_bytes',
              'blockHeight': '100',
              'spent': false,
              'keyImage': 'ki_no_coinbase',
              // 'isCoinbase' intentionally omitted
            },
          ],
        },
      };

      final deserialized = WalletSerializer.deserialize(serialized);

      // isCoinbase should default to false when missing
      expect(deserialized.outputs[0].isCoinbase, false);
      expect(deserialized.outputsByAccount[0]![0].isCoinbase, false);
    });
  });
}
