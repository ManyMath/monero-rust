import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/wallet_lifecycle_manager.dart';
import 'package:monero_extension/services/wallet_persistence_service.dart';
import 'package:monero_extension/src/ffi/signal_hub.dart';
import 'package:monero_extension/src/ffi/signal_sender.dart';
import 'package:monero_extension/src/ffi/signal_types.dart';
import 'package:monero_extension/state/wallet_state.dart';

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
}
