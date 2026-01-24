import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/models/wallet_instance.dart';
import 'package:monero_extension/src/ffi/signal_sender.dart';
import 'package:monero_extension/services/wallet_scan_service.dart';

const _validSeed =
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon abandon abandon abandon abandon abandon abandon';

const _nodeUrl = 'node.monero.com:18081';

WalletInstance _makeWallet({
  String walletId = 'w1',
  String seed = _validSeed,
  String network = 'stagenet',
  String address = '5testaddr',
  List<int>? accounts,
  Set<int>? scanningAccounts,
}) {
  return WalletInstance(
    walletId: walletId,
    seed: seed,
    network: network,
    address: address,
    accounts: accounts,
    scanningAccounts: scanningAccounts,
  );
}

class _RecordedSignal {
  final String name;
  final Map<String, dynamic> data;

  const _RecordedSignal(this.name, this.data);
}

class _RecordingSignalSender implements SignalSender {
  final sent = <_RecordedSignal>[];

  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {
    sent.add(_RecordedSignal(signalName, Map<String, dynamic>.from(data)));
  }

  @override
  Stream<Map<String, dynamic>> onRawSignal(String typeName) =>
      const Stream.empty();
}

void main() {
  group('WalletScanService.validateScanBlock', () {
    test('empty seed returns error', () {
      final result = WalletScanService.validateScanBlock(
        seed: '',
        blockHeight: '100',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('seed phrase'));
    });

    test('invalid seed (wrong word count) returns error', () {
      final result = WalletScanService.validateScanBlock(
        seed: 'one two three four five',
        blockHeight: '100',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid seed phrase'));
      expect(result.error, contains('Expected 12, 16, or 25 words'));
    });

    test('empty block height returns error', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('block height'));
    });

    test('non-numeric height returns error', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: 'abc',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid block height'));
    });

    test('negative height returns error', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '-5',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid block height'));
    });

    test('empty node URL returns error', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '100',
        nodeUrl: '',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test(
      'valid params returns success with normalized seed and parsed height',
      () {
        final result = WalletScanService.validateScanBlock(
          seed: _validSeed,
          blockHeight: '12345',
          nodeUrl: _nodeUrl,
        );
        expect(result.isValid, true);
        expect(result.error, isNull);
        expect(result.normalizedSeed, _validSeed);
        expect(result.blockHeight, 12345);
        expect(result.nodeUrl, 'http://$_nodeUrl');
      },
    );

    test('node URL normalization adds http:// when no scheme present', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '100',
        nodeUrl: 'mynode.local:18081',
      );
      expect(result.isValid, true);
      expect(result.nodeUrl, 'http://mynode.local:18081');
    });

    test('node URL with https:// is preserved', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '100',
        nodeUrl: 'https://secure.node.com:18081',
      );
      expect(result.isValid, true);
      expect(result.nodeUrl, 'https://secure.node.com:18081');
    });

    test('node URL with http:// is preserved', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '100',
        nodeUrl: 'http://plain.node.com:18081',
      );
      expect(result.isValid, true);
      expect(result.nodeUrl, 'http://plain.node.com:18081');
    });

    test('height of 0 is valid', () {
      final result = WalletScanService.validateScanBlock(
        seed: _validSeed,
        blockHeight: '0',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, true);
      expect(result.blockHeight, 0);
    });

    test('seed with extra whitespace is normalized', () {
      final messySeed =
          '  abandon   abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon abandon  ';
      final result = WalletScanService.validateScanBlock(
        seed: messySeed,
        blockHeight: '100',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, true);
      expect(result.normalizedSeed, _validSeed);
    });
  });

  group('WalletScanService.validateContinuousScan', () {
    test('no active wallets and empty seed returns error', () {
      final result = WalletScanService.validateContinuousScan(
        seed: '',
        blockHeight: '100',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, false);
      expect(result.error, contains('seed phrase'));
      expect(result.error, contains('load a wallet'));
    });

    test('no active wallets and invalid seed returns error', () {
      final result = WalletScanService.validateContinuousScan(
        seed: 'just three words',
        blockHeight: '100',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid seed phrase'));
    });

    test('no active wallets but valid seed returns success', () {
      final result = WalletScanService.validateContinuousScan(
        seed: _validSeed,
        blockHeight: '100',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, true);
      expect(result.startHeight, 100);
      expect(result.nodeUrl, 'http://$_nodeUrl');
    });

    test(
      'active wallets with empty seed returns success (wallets do not need seed)',
      () {
        final result = WalletScanService.validateContinuousScan(
          seed: '',
          blockHeight: '500',
          nodeUrl: _nodeUrl,
          activeWallets: [_makeWallet()],
        );
        expect(result.isValid, true);
        expect(result.startHeight, 500);
      },
    );

    test(
      'active wallets with invalid seed still succeeds (seed validation skipped)',
      () {
        final result = WalletScanService.validateContinuousScan(
          seed: 'not a valid seed',
          blockHeight: '500',
          nodeUrl: _nodeUrl,
          activeWallets: [_makeWallet()],
        );
        // When activeWallets is non-empty, seed validation is entirely skipped
        expect(result.isValid, true);
      },
    );

    test('empty block height returns error', () {
      final result = WalletScanService.validateContinuousScan(
        seed: _validSeed,
        blockHeight: '',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, false);
      expect(result.error, contains('block height'));
    });

    test('invalid block height returns error', () {
      final result = WalletScanService.validateContinuousScan(
        seed: _validSeed,
        blockHeight: 'xyz',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid block height'));
    });

    test('negative block height returns error', () {
      final result = WalletScanService.validateContinuousScan(
        seed: _validSeed,
        blockHeight: '-10',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid block height'));
    });

    test('empty node URL returns error', () {
      final result = WalletScanService.validateContinuousScan(
        seed: _validSeed,
        blockHeight: '100',
        nodeUrl: '',
        activeWallets: [],
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('valid params with wallets returns success', () {
      final result = WalletScanService.validateContinuousScan(
        seed: '',
        blockHeight: '3000000',
        nodeUrl: 'https://node.monero.org:18081',
        activeWallets: [_makeWallet()],
      );
      expect(result.isValid, true);
      expect(result.error, isNull);
      expect(result.startHeight, 3000000);
      expect(result.nodeUrl, 'https://node.monero.org:18081');
    });

    test('height of 0 is valid', () {
      final result = WalletScanService.validateContinuousScan(
        seed: _validSeed,
        blockHeight: '0',
        nodeUrl: _nodeUrl,
        activeWallets: [],
      );
      expect(result.isValid, true);
      expect(result.startHeight, 0);
    });
  });

  group('WalletScanService.validateMempoolScan', () {
    test('empty seed returns error', () {
      final result = WalletScanService.validateMempoolScan(
        seed: '',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('seed phrase'));
    });

    test('invalid seed (wrong word count) returns error', () {
      final result = WalletScanService.validateMempoolScan(
        seed: 'word1 word2 word3',
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, false);
      expect(result.error, contains('Invalid seed phrase'));
      expect(result.error, contains('Expected 12, 16, or 25 words'));
    });

    test('empty node URL returns error', () {
      final result = WalletScanService.validateMempoolScan(
        seed: _validSeed,
        nodeUrl: '',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('whitespace-only node URL returns error', () {
      final result = WalletScanService.validateMempoolScan(
        seed: _validSeed,
        nodeUrl: '   ',
      );
      expect(result.isValid, false);
      expect(result.error, contains('node URL'));
    });

    test('valid params returns success', () {
      final result = WalletScanService.validateMempoolScan(
        seed: _validSeed,
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, true);
      expect(result.error, isNull);
      expect(result.normalizedSeed, _validSeed);
      expect(result.nodeUrl, 'http://$_nodeUrl');
    });

    test('node URL normalization adds http:// when missing', () {
      final result = WalletScanService.validateMempoolScan(
        seed: _validSeed,
        nodeUrl: 'mempool.node.local:18081',
      );
      expect(result.isValid, true);
      expect(result.nodeUrl, 'http://mempool.node.local:18081');
    });

    test('16-word polyseed is accepted', () {
      const polyseed =
          'abandon abandon abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon abandon abandon abandon';
      final result = WalletScanService.validateMempoolScan(
        seed: polyseed,
        nodeUrl: _nodeUrl,
      );
      expect(result.isValid, true);
      expect(result.normalizedSeed, polyseed);
    });
  });

  group('WalletScanService.startContinuousScan', () {
    test('single-wallet scan sends selected accounts to Rust', () {
      final sender = _RecordingSignalSender();
      setSignalSender(sender);

      WalletScanService.startContinuousScan(
        nodeUrl: 'http://node:38081',
        startHeight: 123,
        walletsToScan: [
          _makeWallet(accounts: [0, 1, 2], scanningAccounts: {2, 0}),
        ],
        accountLookahead: 2,
        subaddressLookahead: 25,
      );

      expect(sender.sent, hasLength(1));
      expect(sender.sent.single.name, 'send_start_continuous_scan_request');
      expect(sender.sent.single.data['accounts_to_scan'], [0, 2]);
      expect(sender.sent.single.data['account_lookahead'], 2);
      expect(sender.sent.single.data['subaddress_lookahead'], 25);
    });

    test('multi-wallet scan sends each wallet selected accounts to Rust', () {
      final sender = _RecordingSignalSender();
      setSignalSender(sender);

      WalletScanService.startContinuousScan(
        nodeUrl: 'http://node:38081',
        startHeight: 456,
        walletsToScan: [
          _makeWallet(walletId: 'w1', accounts: [0, 1], scanningAccounts: {1}),
          _makeWallet(
            walletId: 'w2',
            accounts: [0, 1, 2],
            scanningAccounts: {2, 0},
          ),
        ],
        subaddressLookahead: 50,
      );

      expect(sender.sent, hasLength(1));
      expect(sender.sent.single.name, 'send_start_multi_wallet_scan_request');
      final wallets = sender.sent.single.data['wallets'] as List<dynamic>;
      expect(wallets, hasLength(2));
      expect((wallets[0] as Map<String, dynamic>)['accounts_to_scan'], [1]);
      expect((wallets[1] as Map<String, dynamic>)['accounts_to_scan'], [0, 2]);
      expect((wallets[1] as Map<String, dynamic>)['subaddress_lookahead'], 50);
    });
  });
}
