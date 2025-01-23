import 'dart:async';
import 'dart:convert';
import 'dart:html' as html;
import 'dart:js_util' as js_util;
import 'package:flutter/foundation.dart';
import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';
import 'wallet_storage_service.dart';
import 'crypto_backend.dart';
import 'wallet_serializer.dart';

/// Service for handling wallet data persistence operations
/// including save, load, import, export, and localStorage management.
class WalletPersistenceService {
  final StorageBackend _storage;
  final CryptoBackend _crypto;

  WalletPersistenceService({
    required StorageBackend storage,
    required CryptoBackend crypto,
  })  : _storage = storage,
        _crypto = crypto;

  static WalletPersistenceService? _default;
  static WalletPersistenceService get _defaultInstance =>
      _default ??= WalletPersistenceService(
        storage: LocalStorageBackend(),
        crypto: RustCryptoBackend(),
      );

  /// Get the localStorage key for a specific wallet ID
  static String getStorageKey(String walletId) =>
      WalletSerializer.getStorageKey(walletId);

  /// Save wallet data to storage with encryption (instance method)
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

  /// Load wallet data from storage with decryption (instance method)
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

  /// List all wallet IDs in storage (instance method)
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

  /// Clear wallet data from storage (instance method)
  void clear(String walletId) {
    final storageKey = getStorageKey(walletId);
    debugPrint('[STORAGE] Clearing data for wallet: $walletId');
    _storage.remove(storageKey);
  }

  /// Check if wallet data exists (instance method)
  bool has(String walletId) {
    final storageKey = getStorageKey(walletId);
    return _storage.containsKey(storageKey);
  }

  /// Get raw encrypted data for a wallet (for export)
  String? getRawData(String walletId) {
    final storageKey = getStorageKey(walletId);
    return _storage.get(storageKey);
  }

  /// Store raw encrypted data for a wallet (for import)
  void setRawData(String walletId, String data) {
    final storageKey = getStorageKey(walletId);
    _storage.set(storageKey, data);
  }

  // --- Static convenience methods (delegate to default instance) ---

  static Future<SaveWalletResult> saveWalletData({
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
  }) =>
      _defaultInstance.save(
        walletId: walletId,
        password: password,
        seed: seed,
        network: network,
        address: address,
        nodeUrl: nodeUrl,
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: continuousScanCurrentHeight,
        selectedOutputs: selectedOutputs,
      );

  static Future<LoadWalletResult> loadWalletData({
    required String walletId,
    required String password,
  }) =>
      _defaultInstance.load(walletId: walletId, password: password);

  static List<String> listAvailableWallets() =>
      _defaultInstance.listWallets();

  static void clearWalletData(String walletId) =>
      _defaultInstance.clear(walletId);

  static bool hasWalletData(String walletId) =>
      _defaultInstance.has(walletId);

  /// Export wallet data as an encrypted file
  static Future<ExportWalletResult> exportWallet({
    required String walletId,
  }) async {
    try {
      final rawData = _defaultInstance.getRawData(walletId);
      if (rawData == null || rawData.isEmpty) {
        return ExportWalletResult.error('No saved data found for this wallet');
      }

      // Generate filename with timestamp
      final now = DateTime.now();
      final timestamp =
          '${now.year}${now.month.toString().padLeft(2, '0')}${now.day.toString().padLeft(2, '0')}-'
          '${now.hour.toString().padLeft(2, '0')}${now.minute.toString().padLeft(2, '0')}${now.second.toString().padLeft(2, '0')}';
      final filename = '${walletId}_$timestamp.monero-wallet';

      // Create blob with wallet data
      final bytes = utf8.encode(rawData);
      final blob = html.Blob([bytes], 'application/octet-stream');

      // Try to use File System Access API for "Save As" dialog (Chrome 86+, Edge 86+)
      bool usedSaveAsDialog = false;
      try {
        if (js_util.hasProperty(html.window, 'showSaveFilePicker')) {
          debugPrint(
              '[EXPORT] Using File System Access API (Save As dialog)');

          final options = js_util.newObject();
          js_util.setProperty(options, 'suggestedName', filename);

          final types = js_util.newObject();
          js_util.setProperty(types, 'description', 'Monero Wallet Files');
          final accept = js_util.newObject();
          js_util.setProperty(
              accept, 'application/octet-stream', ['.monero-wallet']);
          js_util.setProperty(types, 'accept', accept);
          js_util.setProperty(options, 'types', [types]);

          final fileHandlePromise = js_util.callMethod(
            html.window,
            'showSaveFilePicker',
            [options],
          );
          final fileHandle =
              await js_util.promiseToFuture(fileHandlePromise);

          final writablePromise =
              js_util.callMethod(fileHandle, 'createWritable', []);
          final writable = await js_util.promiseToFuture(writablePromise);

          final writePromise =
              js_util.callMethod(writable, 'write', [blob]);
          await js_util.promiseToFuture(writePromise);

          final closePromise =
              js_util.callMethod(writable, 'close', []);
          await js_util.promiseToFuture(closePromise);

          usedSaveAsDialog = true;
          debugPrint('[EXPORT] File saved via Save As dialog');
        }
      } catch (e) {
        if (e.toString().contains('aborted')) {
          debugPrint('[EXPORT] User cancelled save dialog');
          return ExportWalletResult.cancelled();
        }
        debugPrint(
            '[EXPORT] File System Access API not available or failed: $e');
      }

      if (!usedSaveAsDialog) {
        debugPrint('[EXPORT] Using fallback download method');
        final url = html.Url.createObjectUrlFromBlob(blob);
        final anchor = html.AnchorElement(href: url)
          ..setAttribute('download', filename)
          ..click();
        html.Url.revokeObjectUrl(url);
      }

      debugPrint(
          '[EXPORT] Successfully exported wallet: $walletId (method: ${usedSaveAsDialog ? 'Save As dialog' : 'auto-download'})');
      return ExportWalletResult.success(
        filename: filename,
        usedSaveAsDialog: usedSaveAsDialog,
      );
    } catch (e) {
      debugPrint('[EXPORT] Export failed: $e');
      return ExportWalletResult.error('Export failed: $e');
    }
  }

  /// Import wallet data from an encrypted file
  static Future<ImportWalletResult> importWallet({
    required html.File file,
    required String walletId,
    required String password,
    required bool shouldOverwrite,
  }) async {
    try {
      debugPrint(
          '[IMPORT] Selected file: ${file.name} (${file.size} bytes)');

      if (file.size > 10 * 1024 * 1024) {
        return ImportWalletResult.error('File too large (max 10MB)');
      }

      final reader = html.FileReader();
      reader.readAsText(file);
      await reader.onLoad.first;

      final encryptedData = reader.result as String?;
      if (encryptedData == null || encryptedData.isEmpty) {
        return ImportWalletResult.error(
            'File is empty or could not be read');
      }

      debugPrint(
          '[IMPORT] Read ${encryptedData.length} characters from file');

      // Verify decryption
      final jsonString =
          await _defaultInstance._crypto.decrypt(password, encryptedData);
      if (jsonString == null) {
        return ImportWalletResult.error(
            'Failed to decrypt file (wrong password or corrupted file)');
      }

      // Verify JSON is valid
      jsonDecode(jsonString);

      // Store to storage
      _defaultInstance.setRawData(walletId, encryptedData);
      debugPrint('[IMPORT] Stored wallet data for: $walletId');

      debugPrint(
          '[IMPORT] Successfully ${shouldOverwrite ? 'overwritten' : 'imported'} wallet: $walletId');
      return ImportWalletResult.success(
        walletId: walletId,
        wasOverwritten: shouldOverwrite,
      );
    } catch (e) {
      debugPrint('[IMPORT] Import failed: $e');
      return ImportWalletResult.error('Import failed: $e');
    }
  }

  /// Extract suggested wallet ID from filename
  static String extractWalletIdFromFilename(String filename) =>
      WalletSerializer.extractWalletIdFromFilename(filename);
}

/// Result type for save wallet operation
class SaveWalletResult {
  final bool success;
  final String? error;

  SaveWalletResult._({required this.success, this.error});

  factory SaveWalletResult.success() =>
      SaveWalletResult._(success: true);

  factory SaveWalletResult.error(String error) =>
      SaveWalletResult._(success: false, error: error);
}

/// Result type for load wallet operation
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

/// Result type for export wallet operation
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

/// Result type for import wallet operation
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
