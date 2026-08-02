import 'dart:js_interop';
import 'dart:js_interop_unsafe';

import 'wallet_storage_service.dart';

/// The extension `chrome` global, absent when the app is a plain web page.
@JS('chrome')
external _Chrome? get _chrome;

extension type _Chrome._(JSObject _) implements JSObject {
  external _ChromeStorage? get storage;
}

extension type _ChromeStorage._(JSObject _) implements JSObject {
  external _StorageArea? get local;
}

extension type _StorageArea._(JSObject _) implements JSObject {
  /// `get(null)` asks for the whole area, so the key is nullable.
  external JSPromise<JSObject> get(JSAny? key);
  external JSPromise<JSAny?> set(JSObject items);
  external JSPromise<JSAny?> remove(JSAny key);
}

class ChromeStorageBackend implements AsyncStorageBackend {
  static bool get isAvailable => _chrome?.storage?.local != null;

  _StorageArea get _area {
    final local = _chrome?.storage?.local;
    if (local == null) {
      throw StateError('chrome.storage.local is not available');
    }
    return local;
  }

  @override
  Future<String?> get(String key) async {
    final result = await _area.get(key.toJS).toDart;
    final value = result.getProperty<JSAny?>(key.toJS);
    return value.isA<JSString>() ? (value! as JSString).toDart : null;
  }

  @override
  Future<void> set(String key, String value) async {
    final item = JSObject();
    item.setProperty(key.toJS, value.toJS);
    await _area.set(item).toDart;
  }

  @override
  Future<void> remove(String key) async {
    await _area.remove(key.toJS).toDart;
  }

  @override
  Future<bool> containsKey(String key) async => await get(key) != null;

  @override
  Future<List<String>> getKeys() async {
    final result = await _area.get(null).toDart;
    return _objectKeys(result).toDart.map((key) => (key as JSString).toDart).toList();
  }

  @override
  Future<void> atomicSet(String key, String value) async {
    final stagingKey = '${key}_staging';
    final tombstoneKey = '${key}_wip';

    await set(tombstoneKey, '1');
    await set(stagingKey, value);
    await set(key, value);
    await remove(tombstoneKey);
    await remove(stagingKey);
  }

  @override
  Future<void> maybeRecover(String key) async {
    final tombstoneKey = '${key}_wip';
    final stagingKey = '${key}_staging';

    if (!await containsKey(tombstoneKey)) return;

    final staging = await get(stagingKey);
    if (staging != null) {
      await set(key, staging);
    }
    await remove(tombstoneKey);
    await remove(stagingKey);
  }
}

/// `Object.keys`, which `dart:js_interop` has no direct equivalent for.
@JS('Object.keys')
external JSArray<JSAny?> _objectKeys(JSObject object);
