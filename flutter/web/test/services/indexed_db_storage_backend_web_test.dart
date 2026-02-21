@TestOn('browser')
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/indexed_db_storage_backend.dart';

void main() {
  group('IndexedDbStorageBackend', () {
    test('stores lists overwrites and removes wallet entries', () async {
      expect(IndexedDbStorageBackend.isAvailable, true);

      final suffix = DateTime.now().microsecondsSinceEpoch;
      final storage = IndexedDbStorageBackend(
        databaseName: 'monero_wallet_storage_test_$suffix',
      );

      await storage.set('monero_wallet_alpha', 'alpha');
      await storage.set('monero_wallet_beta', 'beta');

      expect(await storage.get('monero_wallet_alpha'), 'alpha');
      expect(await storage.containsKey('monero_wallet_beta'), true);
      expect(await storage.getKeys(), [
        'monero_wallet_alpha',
        'monero_wallet_beta',
      ]);

      await storage.atomicSet('monero_wallet_alpha', 'alpha-updated');
      expect(await storage.get('monero_wallet_alpha'), 'alpha-updated');

      await storage.remove('monero_wallet_alpha');
      expect(await storage.get('monero_wallet_alpha'), isNull);
      expect(await storage.containsKey('monero_wallet_alpha'), false);
      expect(await storage.getKeys(), ['monero_wallet_beta']);

      await storage.remove('monero_wallet_beta');
      expect(await storage.getKeys(), isEmpty);
    });
  });
}
