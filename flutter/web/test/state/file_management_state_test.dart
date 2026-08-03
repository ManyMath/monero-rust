import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/wallet_lifecycle_manager.dart';
import 'package:monero_extension/services/wallet_persistence_service.dart';
import 'package:monero_extension/services/wallet_polling_service.dart';
import 'package:monero_extension/src/ffi/signal_hub.dart';
import 'package:monero_extension/src/ffi/signal_sender.dart';
import 'package:monero_extension/src/ffi/signal_types.dart';
import 'package:monero_extension/src/platform/file_transfer.dart';
import 'package:monero_extension/state/file_management_state.dart';
import 'package:monero_extension/state/output_state.dart';
import 'package:monero_extension/state/scan_state.dart';
import 'package:monero_extension/state/wallet_state.dart';

import '../services/test_backends.dart';
import '../src/platform/fake_file_transfer.dart';

/// Records what goes to Rust and lets a test answer as Rust would.
///
/// One controller per signal type, so answering a key image export cannot accidentally
/// satisfy something else that happens to be listening.
class FakeSignalSender implements SignalSender {
  final sent = <({String name, Map<String, dynamic> data})>[];
  final _controllers = <String, StreamController<Map<String, dynamic>>>{};

  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {
    sent.add((name: signalName, data: data));
  }

  @override
  Stream<Map<String, dynamic>> onRawSignal(String typeName) =>
      _controllerFor(typeName).stream;

  StreamController<Map<String, dynamic>> _controllerFor(String typeName) =>
      _controllers.putIfAbsent(
        typeName,
        () => StreamController<Map<String, dynamic>>.broadcast(),
      );

  void emit(String typeName, Map<String, dynamic> json) =>
      _controllerFor(typeName).add(json);

  bool sentAny(String name) => sent.any((signal) => signal.name == name);

  void dispose() {
    for (final controller in _controllers.values) {
      controller.close();
    }
  }
}

/// A harness holding the four states the file management flows touch.
class _Harness {
  final SignalHub hub;
  final WalletState walletState;
  final OutputState outputState;
  final ScanState scanState;
  final WalletPersistenceService persistence;
  final FileManagementState state;

  _Harness._(
    this.hub,
    this.walletState,
    this.outputState,
    this.scanState,
    this.persistence,
    this.state,
  );

  factory _Harness.create() {
    final hub = SignalHub();
    final persistence = WalletPersistenceService(
      storage: InMemoryStorageBackend(),
      crypto: IdentityCryptoBackend(),
    );
    final walletState = WalletState(
      lifecycle: WalletLifecycleManager(
        persistence: WalletPersistenceService(
          storage: InMemoryStorageBackend(),
          crypto: IdentityCryptoBackend(),
        ),
      ),
      signalHub: hub,
    );
    final outputState = OutputState(walletState: walletState);
    final scanState = ScanState(
      walletState: walletState,
      pollingService: WalletPollingService(),
      signalHub: hub,
    );
    return _Harness._(
      hub,
      walletState,
      outputState,
      scanState,
      persistence,
      FileManagementState(
        walletState: walletState,
        outputState: outputState,
        scanState: scanState,
        persistence: persistence,
      ),
    );
  }

  void dispose() {
    state.dispose();
    scanState.dispose();
    outputState.dispose();
    walletState.dispose();
  }

  /// Open a wallet whose seed is the view-only sentinel the import path requires.
  void openViewOnly({String walletId = 'watch_wallet'}) {
    final seed = 'viewonly:${'a' * 64}:${'b' * 64}';
    walletState.walletId = walletId;
    walletState.seedController.text = seed;
    walletState.openWallet(walletId, seed, 'stagenet', 'address');
  }

  void openFull({String walletId = 'full_wallet'}) {
    const seed = 'a full wallet mnemonic stands in for twenty five words';
    walletState.walletId = walletId;
    walletState.seedController.text = seed;
    walletState.openWallet(walletId, seed, 'stagenet', 'address');
  }
}

void main() {
  late FakeSignalSender sender;
  late FakeFileTransfer transfer;

  setUp(() {
    sender = FakeSignalSender();
    setSignalSender(sender);
    transfer = FakeFileTransfer();
    setFileTransfer(transfer);
  });

  tearDown(() {
    sender.dispose();
    clearFileTransfer();
  });

  group('exporting key images', () {
    test('refuses without a wallet, and asks Rust for nothing', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);

      await harness.state.exportKeyImages();

      expect(
        harness.state.keyImageExportError,
        'No wallet seed available for key image export',
      );
      expect(sender.sentAny('send_export_key_images_request'), isFalse);
      expect(transfer.downloads, isEmpty);
    });

    test('refuses a view-only wallet, which has no spend key to sign with', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openViewOnly();

      await harness.state.exportKeyImages();

      expect(
        harness.state.keyImageExportError,
        'A full wallet is required to export key images',
      );
      expect(sender.sentAny('send_export_key_images_request'), isFalse);
      expect(transfer.downloads, isEmpty);
    });

    test('hands the exported bytes to the user, named for the wallet', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openFull(walletId: 'cold_wallet');

      final done = harness.state.exportKeyImages();
      await Future<void>.delayed(Duration.zero);

      expect(sender.sentAny('send_export_key_images_request'), isTrue);

      sender.emit('KeyImagesExportedResponse', {
        'success': true,
        'key_images_hex': 'deadbeef',
        'count': 2,
      });
      await done;

      expect(harness.state.isExportingKeyImages, isFalse);
      expect(harness.state.keyImageExportError, isNull);
      expect(transfer.downloads, hasLength(1));
      expect(transfer.downloads.single.filename, 'cold_wallet.key-images');
      expect(transfer.downloads.single.bytes, const [0xde, 0xad, 0xbe, 0xef]);
    });

    test('a failed export downloads nothing', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openFull();

      final done = harness.state.exportKeyImages();
      await Future<void>.delayed(Duration.zero);
      sender.emit('KeyImagesExportedResponse', {
        'success': false,
        'error': 'no outputs to export',
        'count': 0,
      });
      await done;

      expect(harness.state.keyImageExportError, 'no outputs to export');
      expect(transfer.downloads, isEmpty);
    });
  });

  group('importing key images', () {
    test('refuses a full wallet, which derives its own key images', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openFull();

      await harness.state.importKeyImages();

      expect(
        harness.state.keyImageImportError,
        'Key image import is intended for view-only wallets',
      );
      expect(transfer.pickCalls, 0);
    });

    test('a cancelled pick leaves the flow exactly as it found it', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openViewOnly();
      transfer.nextPick = null;

      await harness.state.importKeyImages();

      expect(transfer.pickCalls, 1);
      expect(harness.state.isImportingKeyImages, isFalse);
      expect(harness.state.keyImageImportError, isNull);
      expect(sender.sentAny('send_import_key_images_request'), isFalse);
    });

    test('offers a picker filtered to key image files', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openViewOnly();
      transfer.nextPick = null;

      await harness.state.importKeyImages();

      expect(transfer.acceptFilters, const ['.key-images,.bin,*']);
    });

    test('sends the picked bytes as hex and assigns what comes back', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openViewOnly();
      harness.walletState.allOutputs = [
        _output(txHash: 'later', blockHeight: 20),
        _output(txHash: 'earlier', blockHeight: 10),
      ];
      harness.walletState.lifecycle.activeWallet!.outputs =
          harness.walletState.allOutputs;
      transfer.nextPick = (name: 'watch.key-images', bytes: const [0x01, 0xff]);

      final done = harness.state.importKeyImages();
      await Future<void>.delayed(Duration.zero);

      final request = sender.sent.firstWhere(
        (signal) => signal.name == 'send_import_key_images_request',
      );
      expect(request.data['data_hex'], '01ff');

      sender.emit('KeyImagesImportedResponse', {
        'success': true,
        'imported_count': 2,
        'spent_count': 0,
        'key_images': ['ki_earlier', 'ki_later'],
        'spent_key_images': <String>[],
      });
      await done;

      expect(harness.state.isImportingKeyImages, isFalse);
      expect(harness.state.keyImageImportError, isNull);
      // Assignment follows the wallet's export ordering: oldest output first.
      expect(harness.walletState.allOutputs[0].keyImage, 'ki_later');
      expect(harness.walletState.allOutputs[1].keyImage, 'ki_earlier');
    });

    test('a rejected import reports the error and assigns nothing', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openViewOnly();
      harness.walletState.allOutputs = [
        _output(txHash: 'only', blockHeight: 5),
      ];
      harness.walletState.lifecycle.activeWallet!.outputs =
          harness.walletState.allOutputs;
      transfer.nextPick = (name: 'watch.key-images', bytes: const [0x02]);

      final done = harness.state.importKeyImages();
      await Future<void>.delayed(Duration.zero);
      sender.emit('KeyImagesImportedResponse', {
        'success': false,
        'error': 'key images do not match this wallet',
        'imported_count': 0,
        'spent_count': 0,
      });
      await done;

      expect(
        harness.state.keyImageImportError,
        'key images do not match this wallet',
      );
      expect(harness.walletState.allOutputs.single.keyImage, isEmpty);
    });
  });

  group('auto-save', () {
    test('stays off until a key is cached, so it cannot save unencrypted', () async {
      final harness = _Harness.create();
      addTearDown(harness.dispose);
      harness.openFull(walletId: 'saved_wallet');

      harness.state.setAutoSaveEnabled(true);
      await harness.state.autoSaveIfReady();

      expect(await harness.persistence.hasAsync('saved_wallet'), isFalse);
    });

    testWidgets('writes once unlocking has cached a key', (tester) async {
      final harness = await _loadedHarness(tester, walletId: 'stored_wallet');
      addTearDown(harness.dispose);

      harness.state.setAutoSaveEnabled(true);
      harness.walletState.seedController.text = 'edited after unlocking';
      await harness.state.autoSaveIfReady();
      await _settle(tester, harness);

      expect(harness.state.lastSaveTime, isNotNull);
      final reloaded = await harness.persistence.load(
        walletId: 'stored_wallet',
        password: 'hunter2',
      );
      expect(reloaded.seed, 'edited after unlocking');
    });

    testWidgets('never writes the scratch wallet, however it got named', (
      tester,
    ) async {
      final harness = await _loadedHarness(tester, walletId: 'stored_wallet');
      addTearDown(harness.dispose);

      // A wallet the user has not named yet lives under this id, and saving it would
      // leave a stray entry the wallet list has no way to explain.
      harness.walletState.walletId = 'temp_wallet';
      harness.state.setAutoSaveEnabled(true);
      await harness.state.autoSaveIfReady();
      await _settle(tester, harness);

      expect(await harness.persistence.hasAsync('temp_wallet'), isFalse);
    });

    testWidgets('cancelling forgets the cached key', (tester) async {
      final harness = await _loadedHarness(tester, walletId: 'stored_wallet');
      addTearDown(harness.dispose);

      harness.state.setAutoSaveEnabled(true);
      harness.state.cancelAutoSave();
      harness.walletState.seedController.text = 'edited after cancelling';
      await harness.state.autoSaveIfReady();
      await _settle(tester, harness);

      final reloaded = await harness.persistence.load(
        walletId: 'stored_wallet',
        password: 'hunter2',
      );
      expect(reloaded.seed, isNot('edited after cancelling'));
    });
  });

  testWidgets('loading a wallet reads through the injected persistence', (
    tester,
  ) async {
    final harness = await _loadedHarness(tester, walletId: 'stored_wallet');
    addTearDown(harness.dispose);

    expect(harness.state.loadError, isNull);
    expect(harness.state.isLoadingWallet, isFalse);
    expect(harness.walletState.seedController.text, _storedSeed);
    expect(harness.walletState.network, 'stagenet');
    expect(harness.walletState.continuousScanCurrentHeight, 1234);
    // Rust holds no wallet state of its own until it is told to restore one, so a load
    // that skipped this would leave a wallet that cannot spend what it can see.
    expect(sender.sentAny('send_restore_wallet_data_request'), isTrue);
  });

  testWidgets('a wrong password reports a load failure and changes nothing', (
    tester,
  ) async {
    final harness = _Harness.create();
    addTearDown(harness.dispose);
    harness.walletState.walletId = 'absent_wallet';

    final context = await _pumpForContext(tester);
    await harness.state.loadWalletData(context, password: 'wrong');
    await tester.pump();

    expect(harness.state.loadError, isNotNull);
    expect(harness.state.isLoadingWallet, isFalse);
    expect(harness.walletState.seedController.text, isEmpty);
    expect(sender.sentAny('send_restore_wallet_data_request'), isFalse);
  });
}

const _storedSeed = 'a stored mnemonic standing in for twenty five words';

/// Stop the auto-save timer and let the seed field's debounce fire.
///
/// flutter_test fails a test that ends with a timer outstanding, which is the right call:
/// a periodic auto-save that outlives the wallet it was saving is a real leak.
Future<void> _settle(WidgetTester tester, _Harness harness) async {
  harness.state.cancelAutoSave();
  await tester.pump(const Duration(seconds: 1));
}

/// A harness whose wallet has been saved and then unlocked, which is what caches the
/// encryption key auto-save needs.
Future<_Harness> _loadedHarness(
  WidgetTester tester, {
  required String walletId,
}) async {
  final harness = _Harness.create();
  await harness.persistence.save(
    walletId: walletId,
    password: 'hunter2',
    seed: _storedSeed,
    network: 'stagenet',
    address: '5storedAddress',
    nodeUrl: 'http://node.invalid:18081',
    outputs: const [],
    transactions: const [],
    continuousScanCurrentHeight: 1234,
    selectedOutputs: const {},
    accounts: const [0],
    activeAccount: 0,
    scanningAccounts: const {0},
  );
  harness.walletState.walletId = walletId;

  final context = await _pumpForContext(tester);
  await harness.state.loadWalletData(context, password: 'hunter2');
  await tester.pump();
  return harness;
}

/// The load and save flows take a [BuildContext] to raise dialogs from; give them a real
/// one rather than reaching for a widget the app happens to have.
Future<BuildContext> _pumpForContext(WidgetTester tester) async {
  late BuildContext context;
  await tester.pumpWidget(
    MaterialApp(
      home: Builder(
        builder: (builderContext) {
          context = builderContext;
          return const SizedBox.shrink();
        },
      ),
    ),
  );
  return context;
}

OwnedOutput _output({required String txHash, required int blockHeight}) =>
    OwnedOutput(
      txHash: txHash,
      outputIndex: 0,
      amount: 1000,
      amountXmr: '0.000000001000',
      key: 'key_$txHash',
      keyOffset: 'offset_$txHash',
      commitmentMask: 'mask_$txHash',
      receivedOutputBytes: 'bytes_$txHash',
      blockHeight: blockHeight,
      spent: false,
      keyImage: '',
      isCoinbase: false,
      frozen: false,
    );
