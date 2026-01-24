abstract class StorageBackend {
  String? get(String key);
  void set(String key, String value);
  void remove(String key);
  bool containsKey(String key);
  List<String> get keys;

  /// Write a value with the strongest atomicity this backend supports.
  ///
  /// Backends that do not have a write-ahead mechanism can fall back to [set].
  void atomicSet(String key, String value) => set(key, value);

  /// Recover an interrupted [atomicSet], if the backend supports recovery.
  void maybeRecover(String key) {}
}

abstract class AsyncStorageBackend {
  Future<String?> get(String key);
  Future<void> set(String key, String value);
  Future<void> remove(String key);
  Future<bool> containsKey(String key);
  Future<List<String>> getKeys();

  /// Write a value with the strongest atomicity this backend supports.
  ///
  /// Backends that do not have a write-ahead mechanism can fall back to [set].
  Future<void> atomicSet(String key, String value) => set(key, value);

  /// Recover an interrupted [atomicSet], if the backend supports recovery.
  Future<void> maybeRecover(String key) async {}
}

class SyncStorageBackendAdapter implements AsyncStorageBackend {
  final StorageBackend _storage;

  SyncStorageBackendAdapter(this._storage);

  @override
  Future<String?> get(String key) async => _storage.get(key);

  @override
  Future<void> set(String key, String value) async {
    _storage.set(key, value);
  }

  @override
  Future<void> remove(String key) async {
    _storage.remove(key);
  }

  @override
  Future<bool> containsKey(String key) async => _storage.containsKey(key);

  @override
  Future<List<String>> getKeys() async => _storage.keys;

  @override
  Future<void> atomicSet(String key, String value) async {
    _storage.atomicSet(key, value);
  }

  @override
  Future<void> maybeRecover(String key) async {
    _storage.maybeRecover(key);
  }
}
