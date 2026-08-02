@TestOn('browser')
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/services/chrome_storage_backend.dart';
import 'package:monero_extension/services/extension_service.dart';

/// Smoke tests for the hand-written `dart:js_interop` bindings.
///
/// These replaced `dart:js_util` calls, and a passing `flutter build web` proves only that
/// they compile — an `@JS` binding naming a global that isn't there, or an extension type
/// whose member names don't match the JS object, fails at runtime instead. A plain browser
/// has neither `chrome.storage` nor the extension bridge, so what is checked here is the
/// absent path: the bindings resolve, report unavailability, and don't throw on the way.
void main() {
  group('ChromeStorageBackend', () {
    test('reports unavailable outside an extension rather than throwing', () {
      // The chain is chrome?.storage?.local — every link nullable. A binding that named a
      // missing global wrongly would throw here instead of returning false.
      expect(ChromeStorageBackend.isAvailable, isFalse);
    });

    test('using the area anyway fails with a clear error, not a JS type error', () async {
      final backend = ChromeStorageBackend();
      await expectLater(
        backend.get('anything'),
        throwsA(
          isA<StateError>().having(
            (e) => e.message,
            'message',
            contains('chrome.storage.local'),
          ),
        ),
      );
    });
  });

  group('ExtensionService', () {
    test('reports it is not an extension when no bridge was injected', () {
      expect(ExtensionService().isExtension, isFalse);
      expect(ExtensionService().isSidePanel, isFalse);
    });

    test('navigation calls are no-ops without a bridge', () async {
      // These reach through a null bridge; the point is that they return rather than
      // throwing a JS "undefined is not a function".
      await expectLater(ExtensionService().openFullPage(), completes);
      await expectLater(ExtensionService().openSidePanel(), completes);
    });
  });
}
