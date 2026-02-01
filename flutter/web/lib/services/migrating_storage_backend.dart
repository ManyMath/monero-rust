import 'wallet_storage_service.dart';

class MigratingStorageBackend implements AsyncStorageBackend {
  final AsyncStorageBackend primary;
  final StorageBackend legacy;

  MigratingStorageBackend({required this.primary, required this.legacy});

  @override
  Future<String?> get(String key) async {
    final primaryValue = await primary.get(key);
    if (primaryValue != null) return primaryValue;

    final legacyValue = legacy.get(key);
    if (legacyValue == null) return null;

    await primary.set(key, legacyValue);
    _removeLegacyKeyFamily(key);
    return legacyValue;
  }

  @override
  Future<void> set(String key, String value) async {
    await primary.set(key, value);
    _removeLegacyKeyFamily(key);
  }

  @override
  Future<void> remove(String key) async {
    await primary.remove(key);
    _removeLegacyKeyFamily(key);
  }

  @override
  Future<bool> containsKey(String key) async =>
      await primary.containsKey(key) || legacy.containsKey(key);

  @override
  Future<List<String>> getKeys() async {
    final keys = <String>{...await primary.getKeys(), ...legacy.keys};
    final sorted = keys.toList()..sort();
    return sorted;
  }

  @override
  Future<void> atomicSet(String key, String value) async {
    await primary.atomicSet(key, value);
    _removeLegacyKeyFamily(key);
  }

  @override
  Future<void> maybeRecover(String key) async {
    await primary.maybeRecover(key);
    legacy.maybeRecover(key);

    if (!await primary.containsKey(key)) {
      final legacyValue = legacy.get(key);
      if (legacyValue != null) {
        await primary.set(key, legacyValue);
        _removeLegacyKeyFamily(key);
      }
    }
  }

  void _removeLegacyKeyFamily(String key) {
    legacy.remove(key);
    legacy.remove('${key}_staging');
    legacy.remove('${key}_wip');
  }
}
