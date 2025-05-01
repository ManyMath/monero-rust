import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:tuple/tuple.dart';
import 'package:monero_extension/models/wallet_transaction.dart';
import 'package:monero_extension/src/bindings/bindings.dart';
import 'package:monero_extension/services/wallet_serializer.dart';
import '../test_helpers.dart';

void main() {
  group('Wallet Data Format - Output Serialization Roundtrip', () {
    test('preserves all OwnedOutput fields through save/load', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'abc123def456',
        outputIndex: 2,
        amountXmr: '1.234567890123',
        blockHeight: 12345,
        keyImage: 'ki_abc123',
        subaddressIndex: const Tuple2(0, 3),
        paymentId: 'pay_123',
      );

      final saved = WalletSerializer.serialize(
        seed: 'test seed', network: 'stagenet', address: 'addr',
        nodeUrl: 'http://node:38081', outputs: [output],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );

      // Simulate JSON encode/decode (what actually happens during persistence)
      final json = jsonEncode(saved);
      final loaded = WalletSerializer.deserialize(jsonDecode(json));

      expect(loaded.outputs.length, 1);
      final o = loaded.outputs[0];
      expect(o.txHash, output.txHash);
      expect(o.outputIndex, output.outputIndex);
      expect(o.amount.toInt(), output.amount.toInt());
      expect(o.amountXmr, output.amountXmr);
      expect(o.key, output.key);
      expect(o.keyOffset, output.keyOffset);
      expect(o.commitmentMask, output.commitmentMask);
      expect(o.subaddressIndex, isNotNull);
      expect(o.subaddressIndex!.item1, 0);
      expect(o.subaddressIndex!.item2, 3);
      expect(o.paymentId, 'pay_123');
      expect(o.receivedOutputBytes, output.receivedOutputBytes);
      expect(o.blockHeight.toInt(), 12345);
      expect(o.spent, false);
      expect(o.keyImage, 'ki_abc123');
    });

    test('preserves null subaddressIndex and paymentId', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [output],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs[0].subaddressIndex, isNull);
      expect(loaded.outputs[0].paymentId, isNull);
    });

    test('preserves spent flag correctly', () {
      final spentOutput = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '5.0',
        blockHeight: 100, spent: true,
      );
      final unspentOutput = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '3.0',
        blockHeight: 200, spent: false,
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [spentOutput, unspentOutput],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs[0].spent, true);
      expect(loaded.outputs[1].spent, false);
    });

    test('handles large amounts (piconero precision)', () {
      final output = OwnedOutput(
        txHash: 'tx1', outputIndex: 0,
        amount: Uint64(BigInt.parse('999999999999999')), // ~999.999 XMR
        amountXmr: '999.999999999999',
        key: 'k', keyOffset: 'ko', commitmentMask: 'cm',
        subaddressIndex: null, paymentId: null,
        receivedOutputBytes: 'bytes',
        blockHeight: Uint64(BigInt.from(100000)),
        spent: false, keyImage: 'ki',
        isCoinbase: false,
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'mainnet', address: null,
        nodeUrl: 'http://node', outputs: [output],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs[0].amount.toInt(), 999999999999999);
      expect(loaded.outputs[0].blockHeight.toInt(), 100000);
    });

    test('preserves multiple outputs in order', () {
      final outputs = List.generate(5, (i) => TestHelpers.createMockOutput(
        txHash: 'tx_$i', outputIndex: i, amountXmr: '${i + 1}.0',
        blockHeight: 100 + i,
      ));

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: outputs,
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs.length, 5);
      for (var i = 0; i < 5; i++) {
        expect(loaded.outputs[i].txHash, 'tx_$i');
        expect(loaded.outputs[i].outputIndex, i);
      }
    });
  });

  group('Wallet Data Format - Transaction Serialization Roundtrip', () {
    test('preserves transaction with received outputs', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.5', blockHeight: 500,
        subaddressIndex: const Tuple2(0, 1),
      );
      final tx = WalletTransaction(
        txHash: 'tx1', blockHeight: 500, blockTimestamp: 1700000000,
        receivedOutputs: [output], spentKeyImages: [],
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [output],
        transactions: [tx], continuousScanCurrentHeight: 500,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.transactions.length, 1);
      final loadedTx = loaded.transactions[0];
      expect(loadedTx.txHash, 'tx1');
      expect(loadedTx.blockHeight, 500);
      expect(loadedTx.blockTimestamp, 1700000000);
      expect(loadedTx.receivedOutputs.length, 1);
      expect(loadedTx.receivedOutputs[0].txHash, 'tx1');
      expect(loadedTx.receivedOutputs[0].amountXmr, '1.5');
      expect(loadedTx.receivedOutputs[0].subaddressIndex!.item1, 0);
      expect(loadedTx.receivedOutputs[0].subaddressIndex!.item2, 1);
      expect(loadedTx.spentKeyImages, isEmpty);
    });

    test('preserves transaction with spent key images', () {
      final tx = WalletTransaction(
        txHash: 'spend:ki_abc', blockHeight: 600, blockTimestamp: 1700100000,
        receivedOutputs: [], spentKeyImages: ['ki_abc'],
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [],
        transactions: [tx], continuousScanCurrentHeight: 600,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.transactions.length, 1);
      expect(loaded.transactions[0].txHash, 'spend:ki_abc');
      expect(loaded.transactions[0].receivedOutputs, isEmpty);
      expect(loaded.transactions[0].spentKeyImages, ['ki_abc']);
    });

    test('preserves multiple transactions', () {
      final out1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '1.0', blockHeight: 100,
      );
      final out2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '2.0', blockHeight: 200,
      );
      final transactions = [
        WalletTransaction(txHash: 'tx1', blockHeight: 100, blockTimestamp: 1000,
          receivedOutputs: [out1], spentKeyImages: []),
        WalletTransaction(txHash: 'tx2', blockHeight: 200, blockTimestamp: 2000,
          receivedOutputs: [out2], spentKeyImages: ['ki_old']),
        WalletTransaction(txHash: 'spend:ki_old', blockHeight: 200, blockTimestamp: 2000,
          receivedOutputs: [], spentKeyImages: ['ki_old']),
      ];

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [out1, out2],
        transactions: transactions, continuousScanCurrentHeight: 200,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.transactions.length, 3);
    });

    test('handles transaction with null transactions field gracefully', () {
      final data = {
        'version': 1,
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node', 'outputs': [],
        'transactions': null,
        'scanState': {'continuousScanCurrentHeight': 0},
        'selectedOutputs': [],
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };

      final loaded = WalletSerializer.deserialize(data);
      expect(loaded.transactions, isEmpty);
    });
  });

  group('Wallet Data Format - Full Wallet Roundtrip', () {
    test('preserves complete wallet state through save/load cycle', () {
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0',
          blockHeight: 100, subaddressIndex: const Tuple2(0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 1, amountXmr: '0.5',
          blockHeight: 100, subaddressIndex: const Tuple2(0, 1),
          paymentId: 'pay123',
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2', outputIndex: 0, amountXmr: '3.0',
          blockHeight: 200, spent: true, keyImage: 'ki_spent',
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [outputs[0], outputs[1]], spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'tx2', blockHeight: 200, blockTimestamp: 1700100000,
          receivedOutputs: [outputs[2]], spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'spend:ki_spent', blockHeight: 300, blockTimestamp: 1700200000,
          receivedOutputs: [], spentKeyImages: ['ki_spent'],
        ),
      ];
      final selectedOutputs = {'tx1:0', 'tx1:1'};
      const seed = 'hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden';

      final saved = WalletSerializer.serialize(
        seed: seed,
        network: 'stagenet',
        address: '569ubRY6tYfgF3Vpx...',
        nodeUrl: 'http://node.moneroworld.com:38081',
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: 350,
        selectedOutputs: selectedOutputs,
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );

      // Full JSON roundtrip
      final json = jsonEncode(saved);
      final loaded = WalletSerializer.deserialize(jsonDecode(json));

      // Verify all top-level fields
      expect(loaded.seed, seed);
      expect(loaded.network, 'stagenet');
      expect(loaded.address, '569ubRY6tYfgF3Vpx...');
      expect(loaded.nodeUrl, 'http://node.moneroworld.com:38081');
      expect(loaded.continuousScanCurrentHeight, 350);
      expect(loaded.selectedOutputs, {'tx1:0', 'tx1:1'});

      // Verify outputs count and data
      expect(loaded.outputs.length, 3);
      expect(loaded.outputs[0].amountXmr, '10.0');
      expect(loaded.outputs[1].paymentId, 'pay123');
      expect(loaded.outputs[2].spent, true);

      // Verify transactions count and types
      expect(loaded.transactions.length, 3);
      expect(loaded.transactions[0].receivedOutputs.length, 2);
      expect(loaded.transactions[2].spentKeyImages, ['ki_spent']);
    });

    test('handles empty wallet (no outputs, no transactions)', () {
      final saved = WalletSerializer.serialize(
        seed: 'test seed', network: 'stagenet', address: null,
        nodeUrl: 'http://127.0.0.1:38081', outputs: [],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs, isEmpty);
      expect(loaded.transactions, isEmpty);
      expect(loaded.selectedOutputs, isEmpty);
      expect(loaded.continuousScanCurrentHeight, 0);
    });

    test('wallet with many outputs survives roundtrip', () {
      final outputs = List.generate(100, (i) => TestHelpers.createMockOutput(
        txHash: 'tx_${i ~/ 2}',
        outputIndex: i % 2,
        amountXmr: '${(i + 1) * 0.1}',
        blockHeight: 1000 + i,
        spent: i % 5 == 0,
        keyImage: 'ki_$i',
        subaddressIndex: i % 3 == 0 ? Tuple2(0, i) : null,
      ));

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: 'addr',
        nodeUrl: 'http://node', outputs: outputs,
        transactions: [], continuousScanCurrentHeight: 1100,
        selectedOutputs: {'tx_0:0', 'tx_1:0', 'tx_2:0'},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs.length, 100);

      // Spot-check a few
      expect(loaded.outputs[0].spent, true); // i=0, 0%5==0
      expect(loaded.outputs[1].spent, false);
      expect(loaded.outputs[5].spent, true);
      expect(loaded.outputs[0].subaddressIndex, isNotNull); // i=0, 0%3==0
      expect(loaded.outputs[1].subaddressIndex, isNull); // i=1, 1%3!=0
      expect(loaded.selectedOutputs.length, 3);
    });

    test('output amounts survive as exact piconero values', () {
      final output = OwnedOutput(
        txHash: 'tx1', outputIndex: 0,
        amount: Uint64(BigInt.parse('123456789012')),
        amountXmr: '0.123456789012',
        key: 'k', keyOffset: 'ko', commitmentMask: 'cm',
        subaddressIndex: null, paymentId: null,
        receivedOutputBytes: 'bytes',
        blockHeight: Uint64(BigInt.from(500)),
        spent: false, keyImage: 'ki',
        isCoinbase: false,
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [output],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.outputs[0].amount.toInt(), 123456789012);
      expect(loaded.outputs[0].amountXmr, '0.123456789012');
    });
  });

  group('Wallet Data Format - Output inside Transaction vs Top-level', () {
    test('output serialized in transaction matches top-level output', () {
      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '2.5', blockHeight: 500,
        subaddressIndex: const Tuple2(1, 7), paymentId: 'pid',
      );
      final tx = WalletTransaction(
        txHash: 'tx1', blockHeight: 500, blockTimestamp: 1700000000,
        receivedOutputs: [output], spentKeyImages: [],
      );

      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [output],
        transactions: [tx], continuousScanCurrentHeight: 500,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      final topOutput = loaded.outputs[0];
      final txOutput = loaded.transactions[0].receivedOutputs[0];

      // Every field must match between both serialization paths
      expect(topOutput.txHash, txOutput.txHash);
      expect(topOutput.outputIndex, txOutput.outputIndex);
      expect(topOutput.amount.toInt(), txOutput.amount.toInt());
      expect(topOutput.amountXmr, txOutput.amountXmr);
      expect(topOutput.key, txOutput.key);
      expect(topOutput.keyOffset, txOutput.keyOffset);
      expect(topOutput.commitmentMask, txOutput.commitmentMask);
      expect(topOutput.subaddressIndex?.item1, txOutput.subaddressIndex?.item1);
      expect(topOutput.subaddressIndex?.item2, txOutput.subaddressIndex?.item2);
      expect(topOutput.paymentId, txOutput.paymentId);
      expect(topOutput.receivedOutputBytes, txOutput.receivedOutputBytes);
      expect(topOutput.blockHeight.toInt(), txOutput.blockHeight.toInt());
      expect(topOutput.spent, txOutput.spent);
      expect(topOutput.keyImage, txOutput.keyImage);
    });
  });

  group('Wallet Data Format - Selected Outputs', () {
    test('preserves selected outputs set through roundtrip', () {
      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {'tx1:0', 'tx1:1', 'tx2:0'},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.selectedOutputs, {'tx1:0', 'tx1:1', 'tx2:0'});
    });

    test('handles empty selected outputs', () {
      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.selectedOutputs, isEmpty);
    });
  });

  group('Wallet Data Format - Scan State', () {
    test('preserves scan height through roundtrip', () {
      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [],
        transactions: [], continuousScanCurrentHeight: 987654,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.continuousScanCurrentHeight, 987654);
    });

    test('preserves zero scan height', () {
      final saved = WalletSerializer.serialize(
        seed: 'test', network: 'stagenet', address: null,
        nodeUrl: 'http://node', outputs: [],
        transactions: [], continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0], activeAccount: 0, scanningAccounts: {0},
      );
      final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

      expect(loaded.continuousScanCurrentHeight, 0);
    });
  });

  group('Wallet Data Format - Network Types', () {
    for (final network in ['mainnet', 'stagenet', 'testnet']) {
      test('preserves $network', () {
        final saved = WalletSerializer.serialize(
          seed: 'test', network: network, address: null,
          nodeUrl: 'http://node', outputs: [],
          transactions: [], continuousScanCurrentHeight: 0,
          selectedOutputs: {},
          accounts: [0], activeAccount: 0, scanningAccounts: {0},
        );
        final loaded = WalletSerializer.deserialize(jsonDecode(jsonEncode(saved)));

        expect(loaded.network, network);
      });
    }
  });

  group('Wallet Data Format - Deserialization Error Cases', () {
    test('throws on missing version field', () {
      final data = {
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node', 'outputs': [],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 0},
        'selectedOutputs': [],
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };
      expect(() => WalletSerializer.deserialize(data),
          throwsA(isA<FormatException>()));
    });

    test('throws on missing scanState field', () {
      final data = {
        'version': 1,
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node', 'outputs': [],
        'transactions': [],
        'selectedOutputs': [],
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };
      expect(() => WalletSerializer.deserialize(data), throwsA(anything));
    });

    test('throws on missing selectedOutputs field', () {
      final data = {
        'version': 1,
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node', 'outputs': [],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 0},
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };
      expect(() => WalletSerializer.deserialize(data), throwsA(anything));
    });

    test('throws on missing outputs field', () {
      final data = {
        'version': 1,
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node',
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 0},
        'selectedOutputs': [],
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };
      expect(() => WalletSerializer.deserialize(data), throwsA(anything));
    });

    test('throws on malformed output amount', () {
      final data = {
        'version': 1,
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node',
        'outputs': [
          {
            'txHash': 'tx1', 'outputIndex': 0,
            'amount': 'not_a_number', 'amountXmr': '1.0',
            'key': 'k', 'keyOffset': 'ko', 'commitmentMask': 'cm',
            'subaddressIndex': null, 'paymentId': null,
            'receivedOutputBytes': 'bytes',
            'blockHeight': '100', 'spent': false, 'keyImage': 'ki',
          }
        ],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 0},
        'selectedOutputs': [],
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };
      expect(() => WalletSerializer.deserialize(data), throwsA(anything));
    });

    test('throws on empty JSON object', () {
      expect(() => WalletSerializer.deserialize({}), throwsA(anything));
    });

    test('throws on output missing required fields', () {
      final data = {
        'version': 1,
        'seed': 'test', 'network': 'stagenet', 'address': null,
        'nodeUrl': 'http://node',
        'outputs': [{'txHash': 'tx1'}],
        'transactions': [],
        'scanState': {'continuousScanCurrentHeight': 0},
        'selectedOutputs': [],
        'accounts': [0], 'activeAccount': 0, 'scanningAccounts': [0],
      };
      expect(() => WalletSerializer.deserialize(data), throwsA(anything));
    });
  });

  group('WalletSerializer.getStorageKey', () {
    test('prefixes wallet ID correctly', () {
      expect(WalletSerializer.getStorageKey('my-wallet'), 'monero_wallet_my-wallet');
    });

    test('handles empty wallet ID', () {
      expect(WalletSerializer.getStorageKey(''), 'monero_wallet_');
    });

    test('handles special characters in wallet ID', () {
      expect(WalletSerializer.getStorageKey('test_wallet-1'), 'monero_wallet_test_wallet-1');
    });
  });

  group('WalletSerializer.extractWalletIdFromFilename', () {
    test('extracts ID from standard export filename', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          'my-wallet_20260210-143025.monero-wallet'), 'my-wallet');
    });

    test('handles filename without timestamp', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          'my-wallet.monero-wallet'), 'my-wallet');
    });

    test('handles filename without extension', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          'my-wallet_20260210-143025'), 'my-wallet');
    });

    test('handles plain wallet ID', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          'my-wallet'), 'my-wallet');
    });

    test('handles underscore in wallet ID', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          'my_wallet_20260210-143025.monero-wallet'), 'my_wallet');
    });

    test('returns default for empty after stripping', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          '.monero-wallet'), 'imported_wallet');
    });

    test('returns default for invalid characters', () {
      expect(WalletSerializer.extractWalletIdFromFilename(
          'wallet with spaces.monero-wallet'), 'imported_wallet');
    });
  });
}
