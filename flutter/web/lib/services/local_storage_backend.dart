import 'dart:html' as html;
import 'wallet_storage_service.dart';

class LocalStorageBackend implements StorageBackend {
  @override
  String? get(String key) => html.window.localStorage[key];

  @override
  void set(String key, String value) {
    html.window.localStorage[key] = value;
  }

  @override
  void remove(String key) {
    html.window.localStorage.remove(key);
  }

  @override
  bool containsKey(String key) => html.window.localStorage.containsKey(key);

  @override
  List<String> get keys => html.window.localStorage.keys.toList();

  /// Write-ahead atomic set: tombstone -> staging -> live -> clear.
  ///
  /// Staging key: `{key}_staging`, tombstone key: `{key}_wip`.
  /// Recoverable at any step via [maybeRecover].
  @override
  void atomicSet(String key, String value) {
    final stagingKey = '${key}_staging';
    final tombstoneKey = '${key}_wip';

    html.window.localStorage[tombstoneKey] = '1';
    html.window.localStorage[stagingKey] = value;
    html.window.localStorage[key] = value;
    html.window.localStorage.remove(tombstoneKey);
    html.window.localStorage.remove(stagingKey);
  }

  /// Completes any interrupted [atomicSet] detected via tombstone.
  ///
  /// No-op if no tombstone is present.
  @override
  void maybeRecover(String key) {
    final tombstoneKey = '${key}_wip';
    final stagingKey = '${key}_staging';

    if (!html.window.localStorage.containsKey(tombstoneKey)) return;

    final staging = html.window.localStorage[stagingKey];
    if (staging != null) {
      html.window.localStorage[key] = staging;
    }
    html.window.localStorage.remove(tombstoneKey);
    html.window.localStorage.remove(stagingKey);
  }
}
