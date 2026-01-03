import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/utils/offline_signing_import_utils.dart';

void main() {
  group('isSignedMoneroTxSetHex', () {
    test('detects current wallet2 signed txset containers', () {
      expect(
        isSignedMoneroTxSetHex('${signedMoneroTxSetMagicHex}aabbccdd'),
        true,
      );
      expect(
        isSignedMoneroTxSetHex(
          '${signedMoneroTxSetMagicHex.toUpperCase()}AABB',
        ),
        true,
      );
      expect(
        isSignedMoneroTxSetHex('  ${signedMoneroTxSetMagicHex}aabb  '),
        true,
      );
    });

    test('rejects app raw tx blobs and unsigned txsets', () {
      expect(isSignedMoneroTxSetHex('deadbeef'), false);
      expect(
        isSignedMoneroTxSetHex(
          '4d6f6e65726f20756e7369676e65642074782073657405',
        ),
        false,
      );
    });
  });

  group('isUnsignedMoneroTxSetHex', () {
    test('detects current wallet2 unsigned txset containers', () {
      expect(
        isUnsignedMoneroTxSetHex('${unsignedMoneroTxSetMagicHex}aabbccdd'),
        true,
      );
      expect(
        isUnsignedMoneroTxSetHex(
          '${unsignedMoneroTxSetMagicHex.toUpperCase()}AABB',
        ),
        true,
      );
      expect(
        isUnsignedMoneroTxSetHex('  ${unsignedMoneroTxSetMagicHex}aabb  '),
        true,
      );
    });

    test('rejects app raw tx payloads and signed txsets', () {
      expect(isUnsignedMoneroTxSetHex('deadbeef'), false);
      expect(isUnsignedMoneroTxSetHex(signedMoneroTxSetMagicHex), false);
    });
  });

  group('viewKeyHexFromViewOnlySeed', () {
    test('extracts view key from view-only sentinel', () {
      final viewKey = 'a' * 64;
      final spendKey = 'b' * 64;

      expect(
        viewKeyHexFromViewOnlySeed('viewonly:$viewKey:$spendKey'),
        viewKey,
      );
      expect(
        viewKeyHexFromViewOnlySeed('  viewonly:$viewKey:$spendKey  '),
        viewKey,
      );
    });

    test('rejects full-wallet seeds and malformed sentinels', () {
      expect(viewKeyHexFromViewOnlySeed(null), isNull);
      expect(viewKeyHexFromViewOnlySeed('abbey abbey abbey'), isNull);
      expect(viewKeyHexFromViewOnlySeed('viewonly:only-one-part'), isNull);
      expect(viewKeyHexFromViewOnlySeed('viewonly::${'b' * 64}'), isNull);
    });
  });
}
