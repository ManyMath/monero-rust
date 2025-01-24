abstract class StorageBackend {
  String? get(String key);
  void set(String key, String value);
  void remove(String key);
  bool containsKey(String key);
  List<String> get keys;
}
