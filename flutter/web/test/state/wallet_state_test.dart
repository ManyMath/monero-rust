import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/wallet_lifecycle_manager.dart';
import 'package:monero_extension/services/wallet_persistence_service.dart';
import 'package:monero_extension/services/wallet_polling_service.dart';
import 'package:monero_extension/src/ffi/signal_hub.dart';
import 'package:monero_extension/src/ffi/signal_sender.dart';
import 'package:monero_extension/src/ffi/signal_types.dart';
import 'package:monero_extension/state/output_state.dart';
import 'package:monero_extension/state/scan_state.dart';
import 'package:monero_extension/state/transaction_state.dart';
import 'package:monero_extension/state/wallet_state.dart';
import 'package:monero_extension/utils/output_utils.dart';

import '../services/test_backends.dart';

class RecordingSignalSender implements SignalSender {
  final sent = <({String name, Map<String, dynamic> data})>[];
  final _signals = StreamController<Map<String, dynamic>>.broadcast();

  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {
    sent.add((name: signalName, data: data));
  }

  @override
  Stream<Map<String, dynamic>> onRawSignal(String typeName) => _signals.stream;

  void dispose() {
    _signals.close();
  }
}

WalletState createWalletState(SignalHub hub) {
  return WalletState(
    lifecycle: WalletLifecycleManager(
      persistence: WalletPersistenceService(
        storage: InMemoryStorageBackend(),
        crypto: IdentityCryptoBackend(),
      ),
    ),
    signalHub: hub,
  );
}

OwnedOutput testOutput({
  required String txHash,
  required int outputIndex,
  required int blockHeight,
  String keyImage = '',
}) => OwnedOutput(
  txHash: txHash,
  outputIndex: outputIndex,
  amount: 1000,
  amountXmr: '0.000000001000',
  key: 'key_$txHash',
  keyOffset: 'offset_$txHash',
  commitmentMask: 'mask_$txHash',
  receivedOutputBytes: 'bytes_$txHash',
  blockHeight: blockHeight,
  spent: false,
  keyImage: keyImage,
  isCoinbase: false,
  frozen: false,
);

void main() {
  late RecordingSignalSender sender;

  setUp(() {
    sender = RecordingSignalSender();
    setSignalSender(sender);
  });

  tearDown(() {
    sender.dispose();
  });

  test(
    'beginImportedWallet opens imported view-only wallet after keys derive',
    () {
      final hub = SignalHub();
      final state = createWalletState(hub);
      addTearDown(state.dispose);

      final viewSecretKey = 'a' * 64;
      final spendPublicKey = 'b' * 64;
      final viewPublicKey = 'c' * 64;
      final sentinel = 'viewonly:$viewSecretKey:$spendPublicKey';

      state.beginImportedWallet(
        id: 'watch_wallet',
        seed: sentinel,
        walletNetwork: 'mainnet',
      );

      expect(state.seedType, 'view-only');
      expect(state.viewKeyController.text, viewSecretKey);
      expect(state.spendKeyController.text, spendPublicKey);
      expect(state.walletId, 'watch_wallet');
      expect(state.network, 'mainnet');
      expect(
        sender.sent.where(
          (signal) => signal.name == 'send_derive_keys_request',
        ),
        isNotEmpty,
      );

      hub.onKeysDerived!(
        KeysDerivedResponse(
          address: '4viewOnlyAddress',
          secretSpendKey: '',
          secretViewKey: viewSecretKey,
          publicSpendKey: spendPublicKey,
          publicViewKey: viewPublicKey,
          success: true,
        ),
      );

      expect(state.activeWalletId, 'watch_wallet');
      expect(state.activeWallet, isNotNull);
      expect(state.activeWallet!.seed, sentinel);
      expect(state.activeWallet!.network, 'mainnet');
      expect(state.activeWallet!.address, '4viewOnlyAddress');
    },
  );

  test('beginImportedWallet resets view-only mode for mnemonic imports', () {
    final hub = SignalHub();
    final state = createWalletState(hub);
    addTearDown(state.dispose);

    state.restoreViewOnlyStateFromSeed('viewonly:${'a' * 64}:${'b' * 64}');
    expect(state.seedType, 'view-only');

    state.beginImportedWallet(
      id: 'full_wallet',
      seed: 'not a real seed but enough to verify mode reset',
      walletNetwork: 'stagenet',
    );

    expect(state.seedType, '25 word (classic)');
    expect(state.viewKeyController.text, isEmpty);
    expect(state.spendKeyController.text, isEmpty);
    expect(state.walletId, 'full_wallet');
    expect(state.network, 'stagenet');
  });

  test('beginImportedWallet stores imported full wallet keys in memory', () {
    final hub = SignalHub();
    final state = createWalletState(hub);
    addTearDown(state.dispose);

    final spendSecretKey = 'd' * 64;
    final viewSecretKey = 'e' * 64;

    state.beginImportedWallet(
      id: 'cold_keys_wallet',
      seed: 'imported mnemonic words',
      walletNetwork: 'mainnet',
      spendSecretKey: spendSecretKey,
      viewSecretKey: viewSecretKey,
    );

    hub.onKeysDerived!(
      const KeysDerivedResponse(
        address: '4fullWalletAddress',
        secretSpendKey: 'derived_spend',
        secretViewKey: 'derived_view',
        publicSpendKey: 'public_spend',
        publicViewKey: 'public_view',
        success: true,
      ),
    );

    expect(state.activeWalletId, 'cold_keys_wallet');
    expect(state.activeWallet!.importedSpendSecretKey, spendSecretKey);
    expect(state.activeWallet!.importedViewSecretKey, viewSecretKey);
  });

  test('applyImportedKeyImages assigns by wallet export ordering', () {
    final hub = SignalHub();
    final state = createWalletState(hub);
    addTearDown(state.dispose);

    state.openWallet(
      'watch_wallet',
      'viewonly:${'a' * 64}:${'b' * 64}',
      'stagenet',
      'address',
    );
    state.allOutputs = [
      testOutput(txHash: 'later', outputIndex: 0, blockHeight: 20),
      testOutput(txHash: 'earlier_second', outputIndex: 1, blockHeight: 10),
      testOutput(txHash: 'earlier_first', outputIndex: 0, blockHeight: 10),
    ];
    state.lifecycle.activeWallet!.outputs = state.allOutputs;
    state.pendingSpentKeyImages.add('ki_b');

    final count = state.applyImportedKeyImages(['ki_a', 'ki_b', 'ki_c']);

    expect(count, 3);
    expect(state.allOutputs[0].keyImage, 'ki_c');
    expect(state.allOutputs[1].keyImage, 'ki_b');
    expect(state.allOutputs[2].keyImage, 'ki_a');
    expect(state.lifecycle.activeWallet!.outputs[1].keyImage, 'ki_b');
    expect(state.pendingSpentKeyImages, isNot(contains('ki_b')));
  });

  test('imported spent key images mark matching outputs spent', () {
    final hub = SignalHub();
    final state = createWalletState(hub);
    addTearDown(state.dispose);

    state.openWallet(
      'watch_wallet',
      'viewonly:${'a' * 64}:${'b' * 64}',
      'stagenet',
      'address',
    );
    state.allOutputs = [
      testOutput(txHash: 'later', outputIndex: 0, blockHeight: 20),
      testOutput(txHash: 'earlier', outputIndex: 0, blockHeight: 10),
    ];
    state.lifecycle.activeWallet!.outputs = state.allOutputs;
    state.selectedOutputs.addAll({'later:0', 'earlier:0'});

    final assigned = state.applyImportedKeyImages(['ki_earlier', 'ki_later']);
    OutputUtils.markSpentByKeyImages(state.allOutputs, [
      'ki_later',
    ], state.selectedOutputs);

    expect(assigned, 2);
    expect(state.allOutputs[0].keyImage, 'ki_later');
    expect(state.allOutputs[0].spent, true);
    expect(state.allOutputs[1].keyImage, 'ki_earlier');
    expect(state.allOutputs[1].spent, false);
    expect(state.selectedOutputs, {'earlier:0'});
    expect(state.lifecycle.activeWallet!.outputs[0].spent, true);
  });

  test('broadcastSignedBlob records signed txset broadcasts by txid', () {
    final hub = SignalHub();
    final walletState = createWalletState(hub);
    final outputState = OutputState(walletState: walletState);
    final scanState = ScanState(
      walletState: walletState,
      pollingService: WalletPollingService(),
      signalHub: hub,
    );
    final transactionState = TransactionState(
      walletState: walletState,
      outputState: outputState,
      scanState: scanState,
      signalHub: hub,
    );
    addTearDown(transactionState.dispose);
    addTearDown(scanState.dispose);
    addTearDown(outputState.dispose);
    addTearDown(walletState.dispose);

    walletState.openWallet(
      'watch_wallet',
      'viewonly:${'a' * 64}:${'b' * 64}',
      'stagenet',
      'address',
    );
    walletState.nodeUrlController.text = 'http://node:38081';
    walletState.allOutputs = [
      testOutput(
        txHash: 'spent_output_tx',
        outputIndex: 1,
        blockHeight: 42,
        keyImage: 'ki_from_signed_txset',
      ),
    ];
    walletState.selectedOutputs.add('spent_output_tx:1');

    transactionState.broadcastSignedBlob(
      'deadbeef',
      txId: 'signed_txid',
      spentKeyImages: ['ki_from_signed_txset'],
    );

    expect(sender.sent.last.name, 'send_broadcast_transaction_request');
    expect(sender.sent.last.data, containsPair('tx_id', 'signed_txid'));
    expect(
      sender.sent.last.data,
      containsPair('spent_key_images', ['ki_from_signed_txset']),
    );
    expect(
      sender.sent.last.data,
      containsPair('spent_output_hashes', ['spent_output_tx:1']),
    );

    hub.onTransactionBroadcast!(
      const TransactionBroadcastResponse(success: true, txId: 'signed_txid'),
    );

    expect(walletState.allTransactions, hasLength(1));
    expect(walletState.allTransactions.single.txHash, 'signed_txid');
    expect(walletState.allTransactions.single.blockHeight, 0);
    expect(walletState.allTransactions.single.receivedOutputs, isEmpty);
    expect(walletState.allTransactions.single.spentKeyImages, [
      'ki_from_signed_txset',
    ]);
    expect(walletState.pendingSpentKeyImages, contains('ki_from_signed_txset'));
    expect(walletState.selectedOutputs, isEmpty);
  });
}
