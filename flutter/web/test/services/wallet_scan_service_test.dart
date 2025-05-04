import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/models/wallet_instance.dart';
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
}) {
  return WalletInstance(
    walletId: walletId,
    seed: seed,
    network: network,
    address: address,
  );
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
      expect(result.error, contains('Expected 16 or 25 words'));
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

    test('valid params returns success with normalized seed and parsed height', () {
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
    });

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
      final messySeed = '  abandon   abandon abandon abandon abandon '
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

    test('active wallets with empty seed returns success (wallets do not need seed)', () {
      final result = WalletScanService.validateContinuousScan(
        seed: '',
        blockHeight: '500',
        nodeUrl: _nodeUrl,
        activeWallets: [_makeWallet()],
      );
      expect(result.isValid, true);
      expect(result.startHeight, 500);
    });

    test('active wallets with invalid seed still succeeds (seed validation skipped)', () {
      final result = WalletScanService.validateContinuousScan(
        seed: 'not a valid seed',
        blockHeight: '500',
        nodeUrl: _nodeUrl,
        activeWallets: [_makeWallet()],
      );
      // When activeWallets is non-empty, seed validation is entirely skipped
      expect(result.isValid, true);
    });

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
      expect(result.error, contains('Expected 16 or 25 words'));
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
}
