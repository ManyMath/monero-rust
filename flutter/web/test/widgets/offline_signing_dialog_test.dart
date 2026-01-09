import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/src/ffi/signal_sender.dart';
import 'package:monero_extension/src/ffi/signal_types.dart';
import 'package:monero_extension/utils/offline_signing_import_utils.dart';
import 'package:monero_extension/widgets/offline_signing_dialog.dart';

class RecordingSignalSender implements SignalSender {
  final sent = <({String name, Map<String, dynamic> data})>[];
  final _controllers = <String, StreamController<Map<String, dynamic>>>{};

  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {
    sent.add((name: signalName, data: data));
  }

  @override
  Stream<Map<String, dynamic>> onRawSignal(String typeName) =>
      _controller(typeName).stream;

  void emit(String typeName, Map<String, dynamic> data) {
    _controller(typeName).add(data);
  }

  StreamController<Map<String, dynamic>> _controller(String typeName) =>
      _controllers.putIfAbsent(
        typeName,
        () => StreamController<Map<String, dynamic>>.broadcast(),
      );

  Future<void> dispose() async {
    for (final controller in _controllers.values) {
      await controller.close();
    }
  }
}

void main() {
  late RecordingSignalSender sender;

  setUp(() {
    sender = RecordingSignalSender();
    setSignalSender(sender);
  });

  tearDown(() async {
    await sender.dispose();
  });

  testWidgets('returns signer metadata after showing signed QR', (
    tester,
  ) async {
    TransactionSignedOfflineResponse? result;

    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => ElevatedButton(
            onPressed: () async {
              result = await OfflineSigningDialog.show(
                context,
                isViewOnly: false,
                seed: 'cold seed words',
                network: 'mainnet',
              );
            },
            child: const Text('Open'),
          ),
        ),
      ),
    );

    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'abcdef');
    await tester.tap(find.text('From Text'));
    await tester.pump();

    expect(sender.sent.last.name, 'send_sign_unsigned_transaction_request');
    expect(sender.sent.last.data['unsigned_tx_hex'], 'abcdef');

    sender.emit('TransactionSignedOfflineResponse', {
      'success': true,
      'error': null,
      'error_code': null,
      'error_hint': null,
      'error_transient': null,
      'tx_id': 'a' * 64,
      'fee': 1234,
      'tx_blob': 'deadbeef',
      'tx_key': 'b' * 64,
      'tx_key_additional': ['c' * 64],
      'change_outputs': [],
      'spent_key_images': ['ki_1', 'ki_2'],
    });
    await tester.pump();

    expect(find.text('Done'), findsOneWidget);
    await tester.tap(find.text('Done'));
    await tester.pump();

    expect(result, isNotNull);
    expect(result!.txId, 'a' * 64);
    expect(result!.txBlob, 'deadbeef');
    expect(result!.fee, 1234);
    expect(result!.txKey, 'b' * 64);
    expect(result!.txKeyAdditional, ['c' * 64]);
    expect(result!.spentKeyImages, ['ki_1', 'ki_2']);
  });

  testWidgets('passes wallet2 unsigned txsets through to the offline signer', (
    tester,
  ) async {
    TransactionSignedOfflineResponse? result;
    final unsignedTxSetHex = '${unsignedMoneroTxSetMagicHex}aabbccdd';

    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => ElevatedButton(
            onPressed: () async {
              result = await OfflineSigningDialog.show(
                context,
                isViewOnly: false,
                seed: 'cold seed words',
                network: 'mainnet',
              );
            },
            child: const Text('Open'),
          ),
        ),
      ),
    );

    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), unsignedTxSetHex);
    await tester.tap(find.text('From Text'));
    await tester.pump();

    expect(sender.sent.last.name, 'send_sign_unsigned_transaction_request');
    expect(sender.sent.last.data['unsigned_tx_hex'], unsignedTxSetHex);
    expect(sender.sent.last.data['network'], 'mainnet');

    sender.emit('TransactionSignedOfflineResponse', {
      'success': true,
      'error': null,
      'error_code': null,
      'error_hint': null,
      'error_transient': null,
      'tx_id': 'd' * 64,
      'fee': 4321,
      'tx_blob': 'feedface',
      'signed_txset_hex': '${signedMoneroTxSetMagicHex}feedface',
      'tx_key': 'e' * 64,
      'tx_key_additional': ['f' * 64],
      'change_outputs': [],
      'spent_key_images': ['wallet2_ki_0', 'wallet2_ki_20'],
    });
    await tester.pump();

    expect(find.text('Done'), findsOneWidget);
    await tester.tap(find.text('Done'));
    await tester.pump();

    expect(result, isNotNull);
    expect(result!.txId, 'd' * 64);
    expect(result!.txBlob, 'feedface');
    expect(result!.signedTxSetHex, '${signedMoneroTxSetMagicHex}feedface');
    expect(result!.fee, 4321);
    expect(result!.txKey, 'e' * 64);
    expect(result!.txKeyAdditional, ['f' * 64]);
    expect(result!.spentKeyImages, ['wallet2_ki_0', 'wallet2_ki_20']);
  });
}
