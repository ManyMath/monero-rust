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
  bool containsKey(String key) =>
      html.window.localStorage.containsKey(key);

  @override
  List<String> get keys => html.window.localStorage.keys.toList();
}
