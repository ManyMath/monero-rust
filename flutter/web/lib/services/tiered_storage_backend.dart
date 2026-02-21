import 'dart:convert';

import 'wallet_storage_service.dart';

class TieredStorageBackend implements AsyncStorageBackend {
  static const markerPrefix = '__monero_tiered_storage_ref_v1__:';

  final AsyncStorageBackend primary;
  final AsyncStorageBackend secondary;
  final int maxPrimaryValueBytes;

  TieredStorageBackend({
    required this.primary,
    required this.secondary,
    required this.maxPrimaryValueBytes,
  }) {
    if (maxPrimaryValueBytes < 1) {
      throw ArgumentError.value(
        maxPrimaryValueBytes,
        'maxPrimaryValueBytes',
        'must be positive',
      );
    }
  }

  @override
  Future<String?> get(String key) async {
    final value = await primary.get(key);
    if (value == null) return null;
    final ref = _decodeMarker(value);
    if (ref == null) return value;
    return secondary.get(ref);
  }

  @override
  Future<void> set(String key, String value) async {
    if (_fitsPrimary(value)) {
      await primary.set(key, value);
      await secondary.remove(_secondaryKey(key));
      return;
    }

    final secondaryKey = _secondaryKey(key);
    await secondary.set(secondaryKey, value);
    await primary.set(key, _encodeMarker(secondaryKey));
  }

  @override
  Future<void> remove(String key) async {
    final value = await primary.get(key);
    final ref = value == null ? null : _decodeMarker(value);
    await primary.remove(key);
    await secondary.remove(ref ?? _secondaryKey(key));
  }

  @override
  Future<bool> containsKey(String key) async => await get(key) != null;

  @override
  Future<List<String>> getKeys() => primary.getKeys();

  @override
  Future<void> atomicSet(String key, String value) async {
    if (_fitsPrimary(value)) {
      await primary.atomicSet(key, value);
      await secondary.remove(_secondaryKey(key));
      return;
    }

    final secondaryKey = _secondaryKey(key);
    await secondary.atomicSet(secondaryKey, value);
    await primary.atomicSet(key, _encodeMarker(secondaryKey));
  }

  @override
  Future<void> maybeRecover(String key) async {
    await primary.maybeRecover(key);
    final value = await primary.get(key);
    final ref = value == null ? null : _decodeMarker(value);
    if (ref != null) {
      await secondary.maybeRecover(ref);
    }
  }

  bool _fitsPrimary(String value) =>
      utf8.encode(value).length <= maxPrimaryValueBytes;

  static String _secondaryKey(String key) => 'tiered:$key';

  static String _encodeMarker(String secondaryKey) =>
      '$markerPrefix$secondaryKey';

  static String? _decodeMarker(String value) {
    if (!value.startsWith(markerPrefix)) return null;
    final ref = value.substring(markerPrefix.length);
    return ref.isEmpty ? null : ref;
  }
}
