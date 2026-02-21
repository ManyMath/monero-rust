import 'dart:async';
import 'dart:js_interop';

import 'package:web/web.dart';

import 'wallet_storage_service.dart';

class IndexedDbStorageBackend implements AsyncStorageBackend {
  static const defaultDatabaseName = 'monero_wallet_storage';
  static const _storeName = 'wallets';

  final Future<IDBDatabase> _database;

  IndexedDbStorageBackend({String databaseName = defaultDatabaseName})
    : _database = _open(databaseName);

  static bool get isAvailable => window.indexedDB != null;

  @override
  Future<String?> get(String key) async {
    final store = await _store('readonly');
    final value = await _complete(store.get(key.toJS));
    final dartValue = value.dartify();
    return dartValue is String ? dartValue : null;
  }

  @override
  Future<void> set(String key, String value) async {
    final db = await _database;
    final transaction = db.transaction(_storeName.toJS, 'readwrite');
    transaction.objectStore(_storeName).put(value.toJS, key.toJS);
    await _done(transaction);
  }

  @override
  Future<void> remove(String key) async {
    final db = await _database;
    final transaction = db.transaction(_storeName.toJS, 'readwrite');
    transaction.objectStore(_storeName).delete(key.toJS);
    await _done(transaction);
  }

  @override
  Future<bool> containsKey(String key) async => await get(key) != null;

  @override
  Future<List<String>> getKeys() async {
    final store = await _store('readonly');
    final keys = await _complete(store.getAllKeys());
    final dartKeys = keys.dartify();
    if (dartKeys is! List<Object?>) return const [];
    return [
      for (final key in dartKeys)
        if (key is String) key,
    ];
  }

  @override
  Future<void> atomicSet(String key, String value) async {
    await set(key, value);
  }

  @override
  Future<void> maybeRecover(String key) async {}

  Future<IDBObjectStore> _store(String mode) async {
    final db = await _database;
    return db.transaction(_storeName.toJS, mode).objectStore(_storeName);
  }

  static Future<IDBDatabase> _open(String databaseName) async {
    final factory = window.indexedDB;
    if (factory == null) {
      throw StateError('IndexedDB is not available');
    }

    final request = factory.open(databaseName, 1);
    request.onupgradeneeded = ((Event _) {
      final db = request.result! as IDBDatabase;
      if (!db.objectStoreNames.contains(_storeName)) {
        db.createObjectStore(_storeName);
      }
    }).toJS;
    return (await _complete(request))! as IDBDatabase;
  }

  static Future<JSAny?> _complete(IDBRequest request) {
    final completer = Completer<JSAny?>();
    request.onsuccess = ((Event _) => completer.complete(request.result)).toJS;
    request.onerror = ((Event _) {
      completer.completeError(StateError('IndexedDB request failed'));
    }).toJS;
    return completer.future;
  }

  static Future<void> _done(IDBTransaction transaction) {
    final completer = Completer<void>();
    transaction.oncomplete = ((Event _) => completer.complete()).toJS;
    transaction.onerror = ((Event _) {
      completer.completeError(StateError('IndexedDB transaction failed'));
    }).toJS;
    transaction.onabort = ((Event _) {
      completer.completeError(StateError('IndexedDB transaction aborted'));
    }).toJS;
    return completer.future;
  }
}
