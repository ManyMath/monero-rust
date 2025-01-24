import 'dart:convert';
import 'package:flutter/foundation.dart';
import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';
import 'wallet_storage_service.dart';
import 'crypto_backend.dart';
import 'wallet_serializer.dart';

class WalletPersistenceService {
  final StorageBackend _storage;
  final CryptoBackend _crypto;

  WalletPersistenceService({
    required StorageBackend storage,
    required CryptoBackend crypto,
  })  : _storage = storage,
        _crypto = crypto;

  static String getStorageKey(String walletId) =>
      WalletSerializer.getStorageKey(walletId);

  Future<SaveWalletResult> save({
    required String walletId,
    required String password,
    required String seed,
    required String network,
    required String? address,
    required String nodeUrl,
    required List<OwnedOutput> outputs,
    required List<WalletTransaction> transactions,
    required int continuousScanCurrentHeight,
    required Set<String> selectedOutputs,
  }) async {
    try {
      final storageKey = getStorageKey(walletId);
      final walletData = WalletSerializer.serialize(
        seed: seed,
        network: network,
        address: address,
        nodeUrl: nodeUrl,
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: continuousScanCurrentHeight,
        selectedOutputs: selectedOutputs,
      );

      final jsonString = jsonEncode(walletData);
      debugPrint(
          '[SAVE] Serialized ${outputs.length} outputs, ${transactions.length} transactions');

      final encryptedData = await _crypto.encrypt(password, jsonString);
      if (encryptedData == null) {
        return SaveWalletResult.error('Encryption failed');
      }

      _storage.set(storageKey, encryptedData);
      debugPrint('[SAVE] Stored to key: $storageKey');
      return SaveWalletResult.success();
    } catch (e) {
      debugPrint('[SAVE] Error: $e');
      return SaveWalletResult.error('Save failed: $e');
    }
  }

  Future<LoadWalletResult> load({
    required String walletId,
    required String password,
  }) async {
    try {
      final storageKey = getStorageKey(walletId);
      debugPrint('[LOAD] Looking for wallet data at key: $storageKey');

      final encryptedData = _storage.get(storageKey);
      if (encryptedData == null) {
        return LoadWalletResult.error(
            'No stored wallet data found for wallet: $walletId');
      }
      debugPrint('[LOAD] Found encrypted data (${encryptedData.length} chars)');

      final jsonString = await _crypto.decrypt(password, encryptedData);
      if (jsonString == null) {
        return LoadWalletResult.error(
            'Failed to decrypt wallet data (wrong password?)');
      }

      final walletData = jsonDecode(jsonString) as Map<String, dynamic>;
      final parsed = WalletSerializer.deserialize(walletData);

      return LoadWalletResult.success(
        seed: parsed.seed,
        network: parsed.network,
        address: parsed.address,
        nodeUrl: parsed.nodeUrl,
        outputs: parsed.outputs,
        transactions: parsed.transactions,
        continuousScanCurrentHeight: parsed.continuousScanCurrentHeight,
        selectedOutputs: parsed.selectedOutputs,
      );
    } catch (e) {
      debugPrint('[LOAD] Error: $e');
      return LoadWalletResult.error('Failed to parse wallet data: $e');
    }
  }

  List<String> listWallets() {
    debugPrint('[WALLET] Scanning storage for available wallets...');
    final walletIds = <String>[];
    for (final key in _storage.keys) {
      if (key.startsWith('monero_wallet_')) {
        walletIds.add(key.substring('monero_wallet_'.length));
      }
    }
    walletIds.sort();
    debugPrint(
        '[WALLET] Found ${walletIds.length} wallets: ${walletIds.join(', ')}');
    return walletIds;
  }

  void clear(String walletId) {
    final storageKey = getStorageKey(walletId);
    debugPrint('[STORAGE] Clearing data for wallet: $walletId');
    _storage.remove(storageKey);
  }

  bool has(String walletId) {
    final storageKey = getStorageKey(walletId);
    return _storage.containsKey(storageKey);
  }

  String? getRawData(String walletId) {
    final storageKey = getStorageKey(walletId);
    return _storage.get(storageKey);
  }

  void setRawData(String walletId, String data) {
    final storageKey = getStorageKey(walletId);
    _storage.set(storageKey, data);
  }

  Future<String?> decryptRaw(String password, String ciphertext) =>
      _crypto.decrypt(password, ciphertext);
}

class SaveWalletResult {
  final bool success;
  final String? error;

  SaveWalletResult._({required this.success, this.error});

  factory SaveWalletResult.success() =>
      SaveWalletResult._(success: true);

  factory SaveWalletResult.error(String error) =>
      SaveWalletResult._(success: false, error: error);
}

class LoadWalletResult {
  final bool success;
  final String? error;
  final String? seed;
  final String? network;
  final String? address;
  final String? nodeUrl;
  final List<OwnedOutput>? outputs;
  final List<WalletTransaction>? transactions;
  final int? continuousScanCurrentHeight;
  final Set<String>? selectedOutputs;

  LoadWalletResult._({
    required this.success,
    this.error,
    this.seed,
    this.network,
    this.address,
    this.nodeUrl,
    this.outputs,
    this.transactions,
    this.continuousScanCurrentHeight,
    this.selectedOutputs,
  });

  factory LoadWalletResult.success({
    required String seed,
    required String network,
    required String? address,
    required String nodeUrl,
    required List<OwnedOutput> outputs,
    required List<WalletTransaction> transactions,
    required int continuousScanCurrentHeight,
    required Set<String> selectedOutputs,
  }) =>
      LoadWalletResult._(
        success: true,
        seed: seed,
        network: network,
        address: address,
        nodeUrl: nodeUrl,
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: continuousScanCurrentHeight,
        selectedOutputs: selectedOutputs,
      );

  factory LoadWalletResult.error(String error) =>
      LoadWalletResult._(success: false, error: error);
}

class ExportWalletResult {
  final bool success;
  final bool cancelled;
  final String? error;
  final String? filename;
  final bool? usedSaveAsDialog;

  ExportWalletResult._({
    required this.success,
    this.cancelled = false,
    this.error,
    this.filename,
    this.usedSaveAsDialog,
  });

  factory ExportWalletResult.success({
    required String filename,
    required bool usedSaveAsDialog,
  }) =>
      ExportWalletResult._(
        success: true,
        filename: filename,
        usedSaveAsDialog: usedSaveAsDialog,
      );

  factory ExportWalletResult.cancelled() =>
      ExportWalletResult._(success: false, cancelled: true);

  factory ExportWalletResult.error(String error) =>
      ExportWalletResult._(success: false, error: error);
}

class ImportWalletResult {
  final bool success;
  final String? error;
  final String? walletId;
  final bool? wasOverwritten;

  ImportWalletResult._({
    required this.success,
    this.error,
    this.walletId,
    this.wasOverwritten,
  });

  factory ImportWalletResult.success({
    required String walletId,
    required bool wasOverwritten,
  }) =>
      ImportWalletResult._(
        success: true,
        walletId: walletId,
        wasOverwritten: wasOverwritten,
      );

  factory ImportWalletResult.error(String error) =>
      ImportWalletResult._(success: false, error: error);
}
