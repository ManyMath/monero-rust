import 'package:flutter_test/flutter_test.dart';
import 'package:tuple/tuple.dart';
import '../../lib/src/bindings/bindings.dart';
import '../../lib/models/wallet_transaction.dart';
import '../../lib/services/wallet_lifecycle_manager.dart';
import '../../lib/services/wallet_persistence_service.dart';
import '../test_helpers.dart';
import 'test_backends.dart';

WalletLifecycleManager createManager({InMemoryStorageBackend? storage}) {
  return WalletLifecycleManager(
    persistence: WalletPersistenceService(
      storage: storage ?? InMemoryStorageBackend(),
      crypto: IdentityCryptoBackend(),
    ),
  );
}

Future<void> saveTestWallet(
  InMemoryStorageBackend storage, {
  required String walletId,
  int outputCount = 2,
  int txCount = 2,
}) async {
  final svc = WalletPersistenceService(
    storage: storage,
    crypto: IdentityCryptoBackend(),
  );
  final outputs = List.generate(
    outputCount,
    (i) => TestHelpers.createMockOutput(
      txHash: 'tx$i',
      outputIndex: 0,
      amountXmr: '${(i + 1) * 5}.0',
      blockHeight: 100 + i * 100,
      keyImage: 'ki$i',
    ),
  );
  final transactions = List.generate(
    txCount,
    (i) => WalletTransaction(
      txHash: 'tx$i',
      blockHeight: 100 + i * 100,
      blockTimestamp: 1700000000 + i * 100000,
      receivedOutputs: i < outputCount ? [outputs[i]] : [],
      spentKeyImages: [],
    ),
  );
  await svc.save(
    walletId: walletId,
    password: 'pass',
    seed: 'test seed for $walletId',
    network: 'stagenet',
    address: 'addr_$walletId',
    nodeUrl: 'http://node:38081',
    outputs: outputs,
    transactions: transactions,
    continuousScanCurrentHeight: 500,
    selectedOutputs: {'tx0:0'},
  );
}

void main() {
  group('refreshAvailableWallets', () {
    test('updates available wallet list from persistence', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'alpha');
      await saveTestWallet(storage, walletId: 'bravo');

      final mgr = createManager(storage: storage);
      mgr.refreshAvailableWallets();

      expect(mgr.availableWalletIds, ['alpha', 'bravo']);
    });

    test('does NOT auto-select when walletId is empty', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'w1');

      final mgr = createManager(storage: storage);
      expect(mgr.walletId, '');

      mgr.refreshAvailableWallets();

      expect(mgr.walletId, '',
          reason: 'Empty walletId should not be auto-filled by refresh');
      expect(mgr.availableWalletIds, ['w1']);
    });

    test('auto-selects when current non-empty walletId not in list', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'bravo');

      final mgr = createManager(storage: storage);
      mgr.walletId = 'deleted_wallet';

      mgr.refreshAvailableWallets();

      expect(mgr.walletId, 'bravo',
          reason: 'Should auto-select when current wallet was deleted');
    });

    test('keeps current walletId if still in list', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'alpha');
      await saveTestWallet(storage, walletId: 'bravo');

      final mgr = createManager(storage: storage);
      mgr.walletId = 'alpha';

      mgr.refreshAvailableWallets();

      expect(mgr.walletId, 'alpha');
    });
  });

  group('switchWallet routing', () {
    test('returns alreadyCurrent for same wallet', () {
      final mgr = createManager();
      mgr.walletId = 'w1';

      final result = mgr.switchWallet('w1');

      expect(result, SwitchResult.alreadyCurrent);
      expect(mgr.walletId, 'w1');
    });

    test('returns switchedToOpen for open in-memory wallet', () {
      final mgr = createManager();
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
      ];
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.allOutputs = outputs;
      mgr.activeWallet!.outputs = outputs;

      // Switch away
      mgr.walletId = 'other';
      mgr.activeWalletId = null;
      mgr.allOutputs = [];

      final result = mgr.switchWallet('w1');

      expect(result, SwitchResult.switchedToOpen);
      expect(mgr.walletId, 'w1');
      expect(mgr.allOutputs.length, 1);
    });

    test('returns needsLoad when persistence has data', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'w1');

      final mgr = createManager(storage: storage);

      final result = mgr.switchWallet('w1');

      expect(result, SwitchResult.needsLoad);
      expect(mgr.walletId, 'w1');
    });

    test('returns reset when no stored data', () {
      final mgr = createManager();

      final result = mgr.switchWallet('unknown');

      expect(result, SwitchResult.reset);
      expect(mgr.walletId, 'unknown');
      expect(mgr.allOutputs, isEmpty);
      expect(mgr.allTransactions, isEmpty);
    });

    test('skips closed wallet in openWallets and falls through to persistence',
        () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'w1');

      final mgr = createManager(storage: storage);
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.isClosed = true;

      // Switch away
      mgr.walletId = 'other';

      final result = mgr.switchWallet('w1');

      expect(result, SwitchResult.needsLoad,
          reason: 'Closed wallet should not count as switchedToOpen');
    });
  });

  group('switchToWallet', () {
    test('restores outputs from open wallet', () {
      final mgr = createManager();
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
      ];

      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.restoreLoadedData(
        outputs: outputs,
        transactions: [],
        selectedOutputs: {},
        scanHeight: 500,
      );

      // Switch away
      mgr.walletId = 'other';
      mgr.activeWalletId = null;
      mgr.allOutputs = [];

      // Switch back
      final wallet = mgr.switchToWallet('w1');

      expect(wallet, isNotNull);
      expect(mgr.walletId, 'w1');
      expect(mgr.allOutputs.length, 1);
      expect(mgr.continuousScanCurrentHeight, 500);
    });

    test('does NOT restore transactions', () {
      final mgr = createManager();
      final txs = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [], spentKeyImages: [],
        ),
      ];

      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.allTransactions = txs;

      // Switch away
      mgr.walletId = 'other';
      mgr.activeWalletId = null;
      mgr.allTransactions = [];

      // Switch back
      mgr.switchToWallet('w1');

      expect(mgr.allTransactions, isEmpty,
          reason: 'WalletInstance does not store transactions');
    });

    test('returns null for closed wallet', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.isClosed = true;

      final result = mgr.switchToWallet('w1');

      expect(result, isNull);
    });

    test('returns null for non-existent wallet', () {
      final mgr = createManager();

      final result = mgr.switchToWallet('nope');

      expect(result, isNull);
    });
  });

  group('resetWalletState', () {
    test('clears data state without touching wallet identity', () {
      final mgr = createManager();
      mgr.walletId = 'w1';
      mgr.openWallet('w1', 'seed', 'stagenet', 'addr');
      mgr.restoreLoadedData(
        outputs: [
          TestHelpers.createMockOutput(
            txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
          ),
        ],
        transactions: [
          WalletTransaction(
            txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
            receivedOutputs: [], spentKeyImages: [],
          ),
        ],
        selectedOutputs: {'tx1:0'},
        scanHeight: 500,
      );

      mgr.resetWalletState();

      expect(mgr.allOutputs, isEmpty);
      expect(mgr.allTransactions, isEmpty);
      expect(mgr.selectedOutputs, isEmpty);
      expect(mgr.continuousScanCurrentHeight, 0);
      // Identity preserved
      expect(mgr.walletId, 'w1');
      expect(mgr.openWallets.containsKey('w1'), true);
      expect(mgr.activeWalletId, 'w1');
    });
  });

  group('activeWallets getter', () {
    test('returns only non-closed wallets', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      mgr.openWallet('w3', 'seed3', 'stagenet', 'addr3');

      mgr.openWallets['w2']!.isClosed = true;

      final active = mgr.activeWallets;
      expect(active.length, 2);
      expect(active.map((w) => w.walletId).toList()..sort(), ['w1', 'w3']);
    });

    test('returns empty list when all wallets closed', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.isClosed = true;

      expect(mgr.activeWallets, isEmpty);
    });

    test('returns empty list when no wallets open', () {
      final mgr = createManager();
      expect(mgr.activeWallets, isEmpty);
    });
  });

  group('startNewWallet (Bug 2 regression)', () {
    test('clears openWallets map', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      expect(mgr.openWallets.length, 2);

      mgr.startNewWallet();

      expect(mgr.openWallets, isEmpty);
    });

    test('resets walletId to empty', () {
      final mgr = createManager();
      mgr.walletId = 'my_wallet';

      mgr.startNewWallet();

      expect(mgr.walletId, '');
    });

    test('clears activeWalletId', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      expect(mgr.activeWalletId, 'w1');

      mgr.startNewWallet();

      expect(mgr.activeWalletId, isNull);
      expect(mgr.activeWallet, isNull);
    });

    test('clears outputs, transactions, and selectedOutputs', () {
      final mgr = createManager();
      mgr.allOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
      ];
      mgr.allTransactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [], spentKeyImages: [],
        ),
      ];
      mgr.selectedOutputs = {'tx1:0'};
      mgr.continuousScanCurrentHeight = 500;

      mgr.startNewWallet();

      expect(mgr.allOutputs, isEmpty);
      expect(mgr.allTransactions, isEmpty);
      expect(mgr.selectedOutputs, isEmpty);
      expect(mgr.continuousScanCurrentHeight, 0);
    });
  });

  group('openWallet + restoreLoadedData (Bug 3 regression)', () {
    test('openWallet creates instance with empty outputs', () {
      final mgr = createManager();

      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');

      expect(mgr.allOutputs, isEmpty);
      expect(mgr.activeWallet, isNotNull);
      expect(mgr.activeWallet!.outputs, isEmpty);
    });

    test('restoreLoadedData sets outputs after openWallet', () {
      final mgr = createManager();
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2', outputIndex: 0, amountXmr: '5.0', blockHeight: 200,
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [outputs[0]], spentKeyImages: [],
        ),
      ];

      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      expect(mgr.allOutputs, isEmpty, reason: 'openWallet starts empty');

      mgr.restoreLoadedData(
        outputs: outputs,
        transactions: transactions,
        selectedOutputs: {'tx1:0'},
        scanHeight: 500,
      );

      expect(mgr.allOutputs.length, 2);
      expect(mgr.allTransactions.length, 1);
      expect(mgr.selectedOutputs, {'tx1:0'});
      expect(mgr.continuousScanCurrentHeight, 500);
    });

    test('restoreLoadedData also updates activeWallet.outputs', () {
      final mgr = createManager();
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
      ];

      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.restoreLoadedData(
        outputs: outputs,
        transactions: [],
        selectedOutputs: {},
        scanHeight: 500,
        daemonHeight: 1000,
      );

      expect(mgr.activeWallet!.outputs.length, 1,
          reason: 'activeWallet.outputs must be synced with allOutputs');
      expect(identical(mgr.allOutputs, mgr.activeWallet!.outputs), true,
          reason: 'Should be the same list reference');
      expect(mgr.activeWallet!.currentHeight, 500);
      expect(mgr.activeWallet!.daemonHeight, 1000);
    });

    test('pre-existing outputs are wiped by openWallet then restored', () {
      final mgr = createManager();
      final oldOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'old', outputIndex: 0, amountXmr: '1.0', blockHeight: 50,
        ),
      ];
      mgr.allOutputs = oldOutputs;
      expect(mgr.allOutputs.length, 1);

      // openWallet wipes
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      expect(mgr.allOutputs, isEmpty, reason: 'openWallet creates empty list');

      // restoreLoadedData restores
      final newOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'new1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
        TestHelpers.createMockOutput(
          txHash: 'new2', outputIndex: 0, amountXmr: '5.0', blockHeight: 200,
        ),
      ];
      mgr.restoreLoadedData(
        outputs: newOutputs,
        transactions: [],
        selectedOutputs: {},
        scanHeight: 300,
      );

      expect(mgr.allOutputs.length, 2);
      expect(mgr.allOutputs[0].txHash, 'new1');
    });

    test('restoreLoadedData without activeWallet sets fields but skips wallet instance', () {
      final mgr = createManager();
      expect(mgr.activeWallet, isNull);

      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [], spentKeyImages: [],
        ),
      ];

      mgr.restoreLoadedData(
        outputs: outputs,
        transactions: transactions,
        selectedOutputs: {'tx1:0'},
        scanHeight: 500,
        daemonHeight: 1000,
      );

      expect(mgr.allOutputs.length, 1);
      expect(mgr.allTransactions.length, 1);
      expect(mgr.selectedOutputs, {'tx1:0'});
      expect(mgr.continuousScanCurrentHeight, 500);
      expect(mgr.activeWallet, isNull,
          reason: 'No wallet instance to update');
    });
  });

  group('Bug 1 regression: import after delete', () {
    test('refreshAvailableWallets does not auto-select when walletId empty',
        () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'hemlock');

      final mgr = createManager(storage: storage);
      mgr.startNewWallet();
      expect(mgr.walletId, '');

      mgr.refreshAvailableWallets();

      expect(mgr.walletId, '',
          reason: 'Bug 1 fix: empty walletId must not be auto-filled');
      expect(mgr.availableWalletIds, ['hemlock']);
    });

    test('switchWallet after empty-walletId refresh returns needsLoad',
        () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'hemlock');

      final mgr = createManager(storage: storage);
      mgr.startNewWallet();
      mgr.refreshAvailableWallets();
      // walletId is still '' because of Bug 1 fix

      final result = mgr.switchWallet('hemlock');

      expect(result, SwitchResult.needsLoad,
          reason: 'Should load, not return alreadyCurrent');
    });

    test('full delete→import→switch sequence loads correctly', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'hemlock');

      final mgr = createManager(storage: storage);
      // Simulate having the wallet open
      mgr.walletId = 'hemlock';
      mgr.openWallet('hemlock', 'seed', 'stagenet', 'addr');
      mgr.restoreLoadedData(
        outputs: [
          TestHelpers.createMockOutput(
            txHash: 'tx1', outputIndex: 0, amountXmr: '10.0',
            blockHeight: 100,
          ),
        ],
        transactions: [
          WalletTransaction(
            txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
            receivedOutputs: [], spentKeyImages: [],
          ),
        ],
        selectedOutputs: {'tx1:0'},
        scanHeight: 500,
      );

      // 1. Delete
      mgr.startNewWallet();
      expect(mgr.walletId, '');
      expect(mgr.openWallets, isEmpty);

      // 2. Import (data already in storage from saveTestWallet)
      mgr.refreshAvailableWallets();
      expect(mgr.walletId, '', reason: 'Bug 1 fix');

      // 3. Switch to imported wallet
      final result = mgr.switchWallet('hemlock');
      expect(result, SwitchResult.needsLoad);

      // 4. Simulate _loadWalletData: openWallet + restoreLoadedData
      mgr.openWallet('hemlock', 'test seed for hemlock', 'stagenet', 'addr_hemlock');
      mgr.restoreLoadedData(
        outputs: [
          TestHelpers.createMockOutput(
            txHash: 'tx0', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
          ),
          TestHelpers.createMockOutput(
            txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 200,
          ),
        ],
        transactions: [
          WalletTransaction(
            txHash: 'tx0', blockHeight: 100, blockTimestamp: 1700000000,
            receivedOutputs: [], spentKeyImages: [],
          ),
          WalletTransaction(
            txHash: 'tx1', blockHeight: 200, blockTimestamp: 1700100000,
            receivedOutputs: [], spentKeyImages: [],
          ),
        ],
        selectedOutputs: {'tx0:0'},
        scanHeight: 500,
      );

      // Verify full state
      expect(mgr.walletId, 'hemlock');
      expect(mgr.allOutputs.length, 2);
      expect(mgr.allTransactions.length, 2);
      expect(mgr.selectedOutputs, {'tx0:0'});
      expect(mgr.activeWallet!.outputs.length, 2);
    });
  });

  group('Bug 2 regression: stale openWallets after delete', () {
    test('after openWallet + startNewWallet, switchWallet returns needsLoad',
        () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'hemlock');

      final mgr = createManager(storage: storage);

      // Open the wallet (simulating first load)
      mgr.openWallet('hemlock', 'seed', 'stagenet', 'addr');
      mgr.restoreLoadedData(
        outputs: [
          TestHelpers.createMockOutput(
            txHash: 'tx1', outputIndex: 0, amountXmr: '10.0',
            blockHeight: 100,
          ),
        ],
        transactions: [
          WalletTransaction(
            txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
            receivedOutputs: [], spentKeyImages: [],
          ),
        ],
        selectedOutputs: {},
        scanHeight: 500,
      );
      expect(mgr.openWallets.containsKey('hemlock'), true);

      // Delete (startNewWallet clears openWallets)
      mgr.startNewWallet();
      expect(mgr.openWallets.containsKey('hemlock'), false,
          reason: 'Bug 2 fix: startNewWallet must clear openWallets');

      // Re-import and switch
      final result = mgr.switchWallet('hemlock');

      expect(result, SwitchResult.needsLoad,
          reason:
              'Must do full load, not switchToOpen with stale WalletInstance');
    });

    test('stale wallet would have caused switchedToOpen without fix', () {
      final mgr = createManager();

      // Open wallet
      mgr.openWallet('w1', 'seed', 'stagenet', 'addr');
      mgr.allTransactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [], spentKeyImages: [],
        ),
      ];

      // Verify the stale wallet is in openWallets
      expect(mgr.openWallets.containsKey('w1'), true);

      // Switch away without clearing openWallets (simulating old buggy delete)
      mgr.walletId = '';
      mgr.activeWalletId = null;
      mgr.allOutputs = [];
      mgr.allTransactions = [];

      // Switch back — this would hit switchedToOpen because w1 is still in openWallets
      final result = mgr.switchWallet('w1');
      expect(result, SwitchResult.switchedToOpen);
      // Transactions would NOT be restored
      expect(mgr.allTransactions, isEmpty,
          reason: 'switchToWallet does not restore transactions');
    });
  });

  group('Full lifecycle integration', () {
    test(
        'save→delete→reimport→switch→open→restore preserves outputs and transactions',
        () async {
      final storage = InMemoryStorageBackend();
      final persistence = WalletPersistenceService(
        storage: storage,
        crypto: IdentityCryptoBackend(),
      );
      final mgr = WalletLifecycleManager(persistence: persistence);

      // Create test data
      final outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
          subaddressIndex: const Tuple2(0, 0), keyImage: 'ki1',
        ),
        TestHelpers.createMockOutput(
          txHash: 'tx2', outputIndex: 0, amountXmr: '5.0', blockHeight: 200,
          spent: true, keyImage: 'ki2',
        ),
      ];
      final transactions = [
        WalletTransaction(
          txHash: 'tx1', blockHeight: 100, blockTimestamp: 1700000000,
          receivedOutputs: [outputs[0]], spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'tx2', blockHeight: 200, blockTimestamp: 1700100000,
          receivedOutputs: [outputs[1]], spentKeyImages: [],
        ),
        WalletTransaction(
          txHash: 'spend:ki2', blockHeight: 250, blockTimestamp: 1700150000,
          receivedOutputs: [], spentKeyImages: ['ki2'],
        ),
      ];

      // 1. Save
      final saveResult = await persistence.save(
        walletId: 'hemlock',
        password: 'pass',
        seed: 'hemlock seed phrase',
        network: 'stagenet',
        address: 'hemlock_addr',
        nodeUrl: 'http://node:38081',
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: 500,
        selectedOutputs: {'tx1:0'},
      );
      expect(saveResult.success, true);

      // Simulate wallet is open
      mgr.walletId = 'hemlock';
      mgr.openWallet('hemlock', 'hemlock seed phrase', 'stagenet', 'hemlock_addr');
      mgr.restoreLoadedData(
        outputs: outputs,
        transactions: transactions,
        selectedOutputs: {'tx1:0'},
        scanHeight: 500,
      );

      // 2. Export (grab raw blob)
      final exportedBlob = persistence.getRawData('hemlock');
      expect(exportedBlob, isNotNull);

      // 3. Delete
      persistence.clear('hemlock');
      mgr.startNewWallet();
      expect(persistence.has('hemlock'), false);
      expect(mgr.openWallets, isEmpty);
      expect(mgr.allOutputs, isEmpty);
      expect(mgr.allTransactions, isEmpty);

      // 4. Reimport (store raw blob back)
      persistence.setRawData('hemlock', exportedBlob!);
      expect(persistence.has('hemlock'), true);

      // 5. Switch to reimported wallet
      mgr.refreshAvailableWallets();
      expect(mgr.walletId, '', reason: 'Bug 1 fix');

      final result = mgr.switchWallet('hemlock');
      expect(result, SwitchResult.needsLoad);

      // 6. Load from persistence (simulating _loadWalletData)
      final loadResult = await persistence.load(
        walletId: 'hemlock',
        password: 'pass',
      );
      expect(loadResult.success, true);

      mgr.openWallet(
        'hemlock',
        loadResult.seed!,
        loadResult.network!,
        loadResult.address!,
      );
      mgr.restoreLoadedData(
        outputs: loadResult.outputs!,
        transactions: loadResult.transactions!,
        selectedOutputs: loadResult.selectedOutputs!,
        scanHeight: loadResult.continuousScanCurrentHeight!,
      );

      // 7. Verify EVERYTHING survived
      expect(mgr.walletId, 'hemlock');
      expect(mgr.allOutputs.length, 2);
      expect(mgr.allOutputs[0].txHash, 'tx1');
      expect(mgr.allOutputs[0].amountXmr, '10.0');
      expect(mgr.allOutputs[1].spent, true);
      expect(mgr.allTransactions.length, 3);
      expect(mgr.allTransactions[0].txHash, 'tx1');
      expect(mgr.allTransactions[2].spentKeyImages, ['ki2']);
      expect(mgr.selectedOutputs, {'tx1:0'});
      expect(mgr.continuousScanCurrentHeight, 500);
      expect(mgr.activeWallet!.outputs.length, 2);
    });

    test('multiple wallets: open, switch, delete one, reimport', () async {
      final storage = InMemoryStorageBackend();
      await saveTestWallet(storage, walletId: 'alpha');
      await saveTestWallet(storage, walletId: 'bravo');

      final mgr = createManager(storage: storage);

      // Open both wallets
      mgr.openWallet('alpha', 'seed_a', 'stagenet', 'addr_a');
      mgr.restoreLoadedData(
        outputs: [
          TestHelpers.createMockOutput(
            txHash: 'a1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
          ),
        ],
        transactions: [
          WalletTransaction(
            txHash: 'a1', blockHeight: 100, blockTimestamp: 1700000000,
            receivedOutputs: [], spentKeyImages: [],
          ),
        ],
        selectedOutputs: {},
        scanHeight: 300,
      );

      mgr.openWallet('bravo', 'seed_b', 'stagenet', 'addr_b');
      mgr.restoreLoadedData(
        outputs: [
          TestHelpers.createMockOutput(
            txHash: 'b1', outputIndex: 0, amountXmr: '20.0', blockHeight: 200,
          ),
        ],
        transactions: [
          WalletTransaction(
            txHash: 'b1', blockHeight: 200, blockTimestamp: 1700100000,
            receivedOutputs: [], spentKeyImages: [],
          ),
        ],
        selectedOutputs: {},
        scanHeight: 400,
      );

      expect(mgr.openWallets.length, 2);
      expect(mgr.walletId, 'bravo');

      // Switch to alpha
      final switchResult = mgr.switchWallet('alpha');
      expect(switchResult, SwitchResult.switchedToOpen);
      expect(mgr.walletId, 'alpha');
      expect(mgr.allOutputs[0].txHash, 'a1');

      // Delete all (startNewWallet)
      mgr.startNewWallet();
      expect(mgr.openWallets, isEmpty);

      // Reimport alpha
      final reimportResult = mgr.switchWallet('alpha');
      expect(reimportResult, SwitchResult.needsLoad,
          reason: 'Alpha is no longer in openWallets after startNewWallet');
    });
  });

  group('lowestSyncedHeight', () {
    test('returns lowest height among active wallets', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.currentHeight = 500;
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      mgr.openWallets['w2']!.currentHeight = 300;
      mgr.openWallet('w3', 'seed3', 'stagenet', 'addr3');
      mgr.openWallets['w3']!.currentHeight = 700;

      expect(mgr.lowestSyncedHeight, 300);
    });

    test('ignores wallets with currentHeight=0', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.currentHeight = 0;
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      mgr.openWallets['w2']!.currentHeight = 500;

      expect(mgr.lowestSyncedHeight, 500);
    });

    test('ignores closed wallets', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.currentHeight = 100;
      mgr.openWallets['w1']!.isClosed = true;
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      mgr.openWallets['w2']!.currentHeight = 500;

      expect(mgr.lowestSyncedHeight, 500);
    });

    test('returns 0 when no active wallets', () {
      final mgr = createManager();
      expect(mgr.lowestSyncedHeight, 0);
    });

    test('returns 0 when all heights are 0', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');

      expect(mgr.lowestSyncedHeight, 0);
    });
  });

  group('closeWallet', () {
    test('closes active wallet and switches to next', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      mgr.activeWalletId = 'w1';
      mgr.walletId = 'w1';

      final result = mgr.closeWallet('w1');

      expect(result.found, true);
      expect(result.switchedTo, isNotNull);
      expect(result.switchedTo!.walletId, 'w2');
      expect(mgr.activeWalletId, 'w2');
      expect(mgr.walletId, 'w2');
      expect(mgr.openWallets['w1']!.isClosed, true);
      expect(mgr.openWallets['w1']!.isScanning, false);
    });

    test('closes non-active wallet without switching', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr2');
      mgr.activeWalletId = 'w1';
      mgr.walletId = 'w1';

      final result = mgr.closeWallet('w2');

      expect(result.found, true);
      expect(result.switchedTo, isNull);
      expect(mgr.activeWalletId, 'w1');
      expect(mgr.openWallets['w2']!.isClosed, true);
    });

    test('closes last wallet and clears state', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.allOutputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
        ),
      ];

      final result = mgr.closeWallet('w1');

      expect(result.found, true);
      expect(result.switchedTo, isNull);
      expect(mgr.activeWalletId, isNull);
      expect(mgr.allOutputs, isEmpty);
    });

    test('returns notFound for unknown wallet', () {
      final mgr = createManager();

      final result = mgr.closeWallet('nope');

      expect(result.found, false);
    });

    test('marks wallet as not scanning', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr1');
      mgr.openWallets['w1']!.isScanning = true;

      mgr.closeWallet('w1');

      expect(mgr.openWallets['w1']!.isScanning, false);
    });
  });

  group('distributeMultiWalletScanResults', () {
    test('distributes outputs to matching wallets by address', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr_w2');

      final output1 = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
      );
      final output2 = TestHelpers.createMockOutput(
        txHash: 'tx2', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
      );

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'addr_w1', outputs: [output1]),
          WalletScanResult(address: 'addr_w2', outputs: [output2]),
        ],
        blockHeight: 200,
        daemonHeight: 1000,
        spentKeyImages: [],
      );

      expect(mgr.openWallets['w1']!.outputs.length, 1);
      expect(mgr.openWallets['w1']!.outputs[0].txHash, 'tx1');
      expect(mgr.openWallets['w2']!.outputs.length, 1);
      expect(mgr.openWallets['w2']!.outputs[0].txHash, 'tx2');
    });

    test('updates wallet heights', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'addr_w1', outputs: []),
        ],
        blockHeight: 500,
        daemonHeight: 1000,
        spentKeyImages: [],
      );

      expect(mgr.openWallets['w1']!.currentHeight, 500);
      expect(mgr.openWallets['w1']!.daemonHeight, 1000);
    });

    test('does not decrease currentHeight', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');
      mgr.openWallets['w1']!.currentHeight = 800;

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'addr_w1', outputs: []),
        ],
        blockHeight: 500,
        daemonHeight: 1000,
        spentKeyImages: [],
      );

      expect(mgr.openWallets['w1']!.currentHeight, 800,
          reason: 'Should not decrease height');
    });

    test('marks spent outputs across all wallets', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');
      mgr.openWallets['w1']!.outputs = [
        TestHelpers.createMockOutput(
          txHash: 'tx1', outputIndex: 0, amountXmr: '10.0',
          blockHeight: 100, keyImage: 'ki_spent',
        ),
      ];
      mgr.selectedOutputs = {'tx1:0'};

      mgr.distributeMultiWalletScanResults(
        walletResults: [],
        blockHeight: 200,
        daemonHeight: 1000,
        spentKeyImages: ['ki_spent'],
      );

      expect(mgr.openWallets['w1']!.outputs[0].spent, true);
      expect(mgr.selectedOutputs, isEmpty);
    });

    test('refreshes allOutputs from active wallet', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');
      // activeWalletId is now 'w1'

      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
      );

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'addr_w1', outputs: [output]),
        ],
        blockHeight: 200,
        daemonHeight: 1000,
        spentKeyImages: [],
      );

      expect(mgr.allOutputs.length, 1);
      expect(mgr.allOutputs[0].txHash, 'tx1');
    });

    test('ignores results for unknown addresses', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');

      final output = TestHelpers.createMockOutput(
        txHash: 'tx1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
      );

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'unknown_addr', outputs: [output]),
        ],
        blockHeight: 200,
        daemonHeight: 1000,
        spentKeyImages: [],
      );

      expect(mgr.openWallets['w1']!.outputs, isEmpty);
    });

    test('tracks transactions from multi-wallet scan results', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');
      mgr.openWallet('w2', 'seed2', 'stagenet', 'addr_w2');

      final output1 = TestHelpers.createMockOutput(
        txHash: 'tx_multi_1', outputIndex: 0, amountXmr: '10.0', blockHeight: 100,
      );
      final output2 = TestHelpers.createMockOutput(
        txHash: 'tx_multi_2', outputIndex: 0, amountXmr: '5.0', blockHeight: 100,
      );

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'addr_w1', outputs: [output1]),
          WalletScanResult(address: 'addr_w2', outputs: [output2]),
        ],
        blockHeight: 100,
        daemonHeight: 1000,
        spentKeyImages: [],
        blockTimestamp: 1234567890,
      );

      // Verify transactions were created per wallet
      expect(mgr.openWallets['w1']!.transactions.length, 1);
      expect(mgr.openWallets['w2']!.transactions.length, 1);
      expect(mgr.openWallets['w1']!.transactions[0].txHash, 'tx_multi_1');
      expect(mgr.openWallets['w2']!.transactions[0].txHash, 'tx_multi_2');

      // Verify active wallet transactions (w2 is active after last openWallet)
      expect(mgr.allTransactions.length, 1);
      expect(mgr.allTransactions[0].txHash, 'tx_multi_2');

      // Verify transaction details for w2
      final tx2 = mgr.openWallets['w2']!.transactions[0];
      expect(tx2.blockHeight, 100);
      expect(tx2.blockTimestamp, 1234567890);
      expect(tx2.receivedOutputs.length, 1);
      expect(tx2.receivedOutputs[0].txHash, 'tx_multi_2');
    });

    test('tracks spent key images as transactions in multi-wallet scan', () {
      final mgr = createManager();
      mgr.openWallet('w1', 'seed1', 'stagenet', 'addr_w1');

      // First add an output with a key image
      final output1 = TestHelpers.createMockOutput(
        txHash: 'tx_received', outputIndex: 0, amountXmr: '10.0',
        blockHeight: 100, keyImage: 'ki_spend_1',
      );

      mgr.distributeMultiWalletScanResults(
        walletResults: [
          WalletScanResult(address: 'addr_w1', outputs: [output1]),
        ],
        blockHeight: 100,
        daemonHeight: 1000,
        spentKeyImages: [],
      );

      expect(mgr.openWallets['w1']!.transactions.length, 1);

      // Now scan a block that spends this output
      mgr.distributeMultiWalletScanResults(
        walletResults: [],
        blockHeight: 150,
        daemonHeight: 1000,
        spentKeyImages: ['ki_spend_1'],
      );

      // Should create a synthetic spend transaction in w1
      expect(mgr.openWallets['w1']!.transactions.length, 2);
      final spendTx = mgr.openWallets['w1']!.transactions.firstWhere((t) => t.txHash.startsWith('spend:'));
      expect(spendTx.spentKeyImages, contains('ki_spend_1'));
      expect(spendTx.blockHeight, 150);
    });
  });
}
