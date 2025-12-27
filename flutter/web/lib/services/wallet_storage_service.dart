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
