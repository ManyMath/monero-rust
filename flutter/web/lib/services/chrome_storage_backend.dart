import 'dart:html' as html;
import 'dart:js_util' as js_util;
import 'wallet_storage_service.dart';

class ChromeStorageBackend implements AsyncStorageBackend {
  static bool get isAvailable {
    final chrome = js_util.getProperty<Object?>(html.window, 'chrome');
    if (chrome == null) return false;
    final storage = js_util.getProperty<Object?>(chrome, 'storage');
    if (storage == null) return false;
    return js_util.getProperty<Object?>(storage, 'local') != null;
  }

  Object get _area {
    final chrome = js_util.getProperty<Object?>(html.window, 'chrome');
    final storage = chrome == null
        ? null
        : js_util.getProperty<Object?>(chrome, 'storage');
    final local = storage == null
        ? null
        : js_util.getProperty<Object?>(storage, 'local');
    if (local == null) {
      throw StateError('chrome.storage.local is not available');
    }
    return local;
  }

  @override
  Future<String?> get(String key) async {
    final result = await js_util.promiseToFuture<Object>(
      js_util.callMethod(_area, 'get', [key]),
    );
    final value = js_util.getProperty<Object?>(result, key);
    return value is String ? value : null;
  }

  @override
  Future<void> set(String key, String value) async {
    final item = js_util.newObject();
    js_util.setProperty(item, key, value);
    await js_util.promiseToFuture<void>(
      js_util.callMethod(_area, 'set', [item]),
    );
  }

  @override
  Future<void> remove(String key) async {
    await js_util.promiseToFuture<void>(
      js_util.callMethod(_area, 'remove', [key]),
    );
  }

  @override
  Future<bool> containsKey(String key) async => await get(key) != null;

  @override
  Future<List<String>> getKeys() async {
    final result = await js_util.promiseToFuture<Object>(
      js_util.callMethod(_area, 'get', [null]),
    );
    return js_util.objectKeys(result).cast<String>();
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
