import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/models/wallet_transaction.dart';
import 'package:monero_extension/services/wallet_persistence_service.dart';
import '../test_helpers.dart';
import 'test_backends.dart';

WalletPersistenceService createTestService({
  InMemoryStorageBackend? storage,
  bool failCrypto = false,
}) {
  return WalletPersistenceService(
    storage: storage ?? InMemoryStorageBackend(),
    crypto: failCrypto ? FailingCryptoBackend() : IdentityCryptoBackend(),
  );
}

void main() {
  group('WalletPersistenceService - Async storage backend', () {
    test('save load list export import and clear use async storage', () async {
      final storage = AsyncInMemoryStorageBackend();
      final svc = WalletPersistenceService.async(
        storage: storage,
        crypto: IdentityCryptoBackend(),
      );
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx-async',
          outputIndex: 0,
          amountXmr: '3.0',
          blockHeight: 42,
          subaddressIndex: (1, 4),
        ),
      ];

      final saveResult = await svc.save(
        walletId: 'async-wallet',
        password: 'pass123',
        seed: 'async seed phrase',
        network: 'stagenet',
        address: '5async...',
        nodeUrl: 'http://node:38081',
        outputs: outputs,
        transactions: [],
        continuousScanCurrentHeight: 43,
        selectedOutputs: {'tx-async:0'},
        accounts: [0, 1],
        activeAccount: 1,
        scanningAccounts: {1},
      );
      expect(saveResult.success, true);
      expect(await svc.hasAsync('async-wallet'), true);
      expect(await svc.listWalletsAsync(), ['async-wallet']);

      final exportedBlob = await svc.getRawDataAsync('async-wallet');
      expect(exportedBlob, isNotNull);

      final loadResult = await svc.load(
        walletId: 'async-wallet',
        password: 'pass123',
      );
      expect(loadResult.success, true);
      expect(loadResult.seed, 'async seed phrase');
      expect(loadResult.outputs, hasLength(1));
      expect(loadResult.outputs!.single.subaddressIndex, (1, 4));
      expect(loadResult.scanningAccounts, {1});

      await svc.clearAsync('async-wallet');
      expect(await svc.hasAsync('async-wallet'), false);

      await svc.setRawDataAsync('imported-async-wallet', exportedBlob!);
      expect(await svc.listWalletsAsync(), ['imported-async-wallet']);
    });

    test('sync helpers fail clearly for async-only storage', () {
      final svc = WalletPersistenceService.async(
        storage: AsyncInMemoryStorageBackend(),
        crypto: IdentityCryptoBackend(),
      );

      expect(svc.listWallets, throwsStateError);
      expect(() => svc.has('async-wallet'), throwsStateError);
      expect(() => svc.clear('async-wallet'), throwsStateError);
      expect(() => svc.getRawData('async-wallet'), throwsStateError);
      expect(() => svc.setRawData('async-wallet', 'data'), throwsStateError);
    });
  });

  group('WalletPersistenceService - Save and Load', () {
    test('save then load preserves all wallet data', () async {
      final svc = createTestService();
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '10.0',
          blockHeight: 100,
          subaddressIndex: (0, 0),
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '5.0',
          blockHeight: 200,
          spent: true,
          keyImage: 'ki_spent',
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1',
          blockHeight: 100,
          blockTimestamp: 1700000000,
          receivedOutputs: [outputs[0]],
          spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'spend:ki_spent',
          blockHeight: 250,
          blockTimestamp: 1700100000,
          receivedOutputs: [],
          spentKeyImages: ['ki_spent'],
        ),
      ];

      final saveResult = await svc.save(
        walletId: 'test-wallet',
        password: 'pass123',
        seed: 'test seed phrase',
        network: 'stagenet',
        address: '5addr...',
        nodeUrl: 'http://node:38081',
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: 300,
        selectedOutputs: {'tx1:0'},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );
      expect(saveResult.success, true);

      final loadResult = await svc.load(
        walletId: 'test-wallet',
        password: 'pass123',
      );
      expect(loadResult.success, true);
      expect(loadResult.seed, 'test seed phrase');
      expect(loadResult.network, 'stagenet');
      expect(loadResult.address, '5addr...');
      expect(loadResult.nodeUrl, 'http://node:38081');
      expect(loadResult.outputs!.length, 2);
      expect(loadResult.outputs![0].amountXmr, '10.0');
      expect(loadResult.outputs![1].spent, true);
      expect(loadResult.transactions!.length, 2);
      expect(loadResult.transactions![1].spentKeyImages, ['ki_spent']);
      expect(loadResult.continuousScanCurrentHeight, 300);
      expect(loadResult.selectedOutputs, {'tx1:0'});
    });

    test('load returns error for non-existent wallet', () async {
      final svc = createTestService();
      final result = await svc.load(walletId: 'nope', password: 'pass');
      expect(result.success, false);
      expect(result.error, contains('No stored wallet data'));
    });

    test('save returns error when encryption fails', () async {
      final svc = createTestService(failCrypto: true);
      final result = await svc.save(
        walletId: 'w',
        password: 'p',
        seed: 's',
        network: 'stagenet',
        address: null,
        nodeUrl: 'http://n',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );
      expect(result.success, false);
      expect(result.error, contains('Encryption failed'));
    });

    test('load returns error when decryption fails', () async {
      final storage = InMemoryStorageBackend();
      // Manually put some data in storage
      storage.set('monero_wallet_w', 'encrypted_blob');
      final svc = WalletPersistenceService(
        storage: storage,
        crypto: FailingCryptoBackend(),
      );

      final result = await svc.load(walletId: 'w', password: 'p');
      expect(result.success, false);
      expect(result.error, contains('wrong password'));
    });
  });

  group('WalletPersistenceService - Save→Delete→Import Roundtrip', () {
    test('full save→export→delete→import cycle preserves data', () async {
      final storage = InMemoryStorageBackend();
      final svc = createTestService(storage: storage);

      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1',
          outputIndex: 0,
          amountXmr: '7.5',
          blockHeight: 500,
          keyImage: 'ki1',
          subaddressIndex: (0, 2),
          paymentId: 'pid1',
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2',
          outputIndex: 0,
          amountXmr: '2.5',
          blockHeight: 600,
          spent: true,
          keyImage: 'ki2',
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1',
          blockHeight: 500,
          blockTimestamp: 1700000000,
          receivedOutputs: [outputs[0]],
          spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'tx2',
          blockHeight: 600,
          blockTimestamp: 1700100000,
          receivedOutputs: [outputs[1]],
          spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'spend:ki2',
          blockHeight: 650,
          blockTimestamp: 1700150000,
          receivedOutputs: [],
          spentKeyImages: ['ki2'],
        ),
      ];

      // 1. Save
      final saveResult = await svc.save(
        walletId: 'roundtrip-wallet',
        password: 'mypass',
        seed: 'round trip seed phrase words',
        network: 'mainnet',
        address: '4MainAddr...',
        nodeUrl: 'http://mainnet-node:18081',
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: 700,
        selectedOutputs: {'tx1:0'},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );
      expect(saveResult.success, true);

      // 2. Export (grab the raw encrypted blob)
      final exportedBlob = svc.getRawData('roundtrip-wallet');
      expect(exportedBlob, isNotNull);

      // 3. Delete
      svc.clear('roundtrip-wallet');
      expect(svc.has('roundtrip-wallet'), false);

      // Verify load fails after delete
      final loadAfterDelete = await svc.load(
        walletId: 'roundtrip-wallet',
        password: 'mypass',
      );
      expect(loadAfterDelete.success, false);

      // 4. Import (re-store the blob under a new ID, simulating file import)
      svc.setRawData('imported-wallet', exportedBlob!);
      expect(svc.has('imported-wallet'), true);

      // 5. Load from the imported wallet
      final loadResult = await svc.load(
        walletId: 'imported-wallet',
        password: 'mypass',
      );
      expect(loadResult.success, true);

      // 6. Verify ALL data survived the roundtrip
      expect(loadResult.seed, 'round trip seed phrase words');
      expect(loadResult.network, 'mainnet');
      expect(loadResult.address, '4MainAddr...');
      expect(loadResult.nodeUrl, 'http://mainnet-node:18081');
      expect(loadResult.continuousScanCurrentHeight, 700);
      expect(loadResult.selectedOutputs, {'tx1:0'});

      // Verify outputs
      final loadedOutputs = loadResult.outputs!;
      expect(loadedOutputs.length, 2);
      expect(loadedOutputs[0].txHash, 'tx1');
      expect(loadedOutputs[0].amountXmr, '7.5');
      expect(loadedOutputs[0].keyImage, 'ki1');
      expect(loadedOutputs[0].subaddressIndex!.$1, 0);
      expect(loadedOutputs[0].subaddressIndex!.$2, 2);
      expect(loadedOutputs[0].paymentId, 'pid1');
      expect(loadedOutputs[0].spent, false);
      expect(loadedOutputs[1].txHash, 'tx2');
      expect(loadedOutputs[1].spent, true);

      // Verify transactions
      final loadedTxs = loadResult.transactions!;
      expect(loadedTxs.length, 3);
      expect(loadedTxs[0].txHash, 'tx1');
      expect(loadedTxs[0].receivedOutputs.length, 1);
      expect(loadedTxs[2].txHash, 'spend:ki2');
      expect(loadedTxs[2].spentKeyImages, ['ki2']);
    });

    test('overwrite replaces existing wallet data', () async {
      final storage = InMemoryStorageBackend();
      final svc = createTestService(storage: storage);

      // Save wallet A
      await svc.save(
        walletId: 'shared-id',
        password: 'p',
        seed: 'seed A',
        network: 'stagenet',
        address: null,
        nodeUrl: 'http://node',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 100,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      // Save wallet B under the same ID (overwrite)
      await svc.save(
        walletId: 'shared-id',
        password: 'p',
        seed: 'seed B',
        network: 'mainnet',
        address: 'addr_B',
        nodeUrl: 'http://other-node',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 200,
        selectedOutputs: {'out1'},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );

      final result = await svc.load(walletId: 'shared-id', password: 'p');
      expect(result.success, true);
      expect(result.seed, 'seed B');
      expect(result.network, 'mainnet');
      expect(result.continuousScanCurrentHeight, 200);
    });
  });

  group('WalletPersistenceService - Wallet Management', () {
    test('listWallets returns sorted wallet IDs', () {
      final storage = InMemoryStorageBackend();
      storage.set('monero_wallet_charlie', 'data');
      storage.set('monero_wallet_alpha', 'data');
      storage.set('monero_wallet_bravo', 'data');
      storage.set('other_key', 'not a wallet');

      final svc = WalletPersistenceService(
        storage: storage,
        crypto: IdentityCryptoBackend(),
      );

      final wallets = svc.listWallets();
      expect(wallets, ['alpha', 'bravo', 'charlie']);
    });

    test('listWallets returns empty list when no wallets', () {
      final svc = createTestService();
      expect(svc.listWallets(), isEmpty);
    });

    test('has returns true for existing wallet', () async {
      final svc = createTestService();
      await svc.save(
        walletId: 'exists',
        password: 'p',
        seed: 's',
        network: 'stagenet',
        address: null,
        nodeUrl: 'http://n',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );
      expect(svc.has('exists'), true);
      expect(svc.has('nope'), false);
    });

    test('clear removes wallet data', () async {
      final svc = createTestService();
      await svc.save(
        walletId: 'to-delete',
        password: 'p',
        seed: 's',
        network: 'stagenet',
        address: null,
        nodeUrl: 'http://n',
        outputs: [],
        transactions: [],
        continuousScanCurrentHeight: 0,
        selectedOutputs: {},
        accounts: [0],
        activeAccount: 0,
        scanningAccounts: {0},
      );
      expect(svc.has('to-delete'), true);

      svc.clear('to-delete');
      expect(svc.has('to-delete'), false);
    });

    test('clear is idempotent for non-existent wallet', () {
      final svc = createTestService();
      // Should not throw
      svc.clear('nonexistent');
      expect(svc.has('nonexistent'), false);
    });
  });
}
