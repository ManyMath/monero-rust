import 'dart:convert';
import 'dart:html' as html;
import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import '../src/ffi/signal_types.dart';
import '../models/wallet_transaction.dart';
import 'chrome_storage_backend.dart';
import 'indexed_db_storage_backend.dart';
import 'wallet_persistence_service.dart';
import 'local_storage_backend.dart';
import 'migrating_storage_backend.dart';
import 'rust_crypto_backend.dart';
import 'tiered_storage_backend.dart';
import 'wallet_serializer.dart';

export 'wallet_persistence_service.dart';

class WalletPersistenceBrowser {
  static WalletPersistenceService? _default;
  static WalletPersistenceService get defaultPersistence => _defaultInstance;
  static WalletPersistenceService get _defaultInstance =>
      _default ??= _createDefaultPersistence();

  static WalletPersistenceService _createDefaultPersistence() {
    final crypto = RustCryptoBackend();
    if (ChromeStorageBackend.isAvailable) {
      final chromeStorage = ChromeStorageBackend();
      final primary = IndexedDbStorageBackend.isAvailable
          ? TieredStorageBackend(
              primary: chromeStorage,
              secondary: IndexedDbStorageBackend(),
              maxPrimaryValueBytes: 512 * 1024,
            )
          : chromeStorage;
      return WalletPersistenceService.async(
        storage: MigratingStorageBackend(
          primary: primary,
          legacy: LocalStorageBackend(),
        ),
        crypto: crypto,
      );
    }
    return WalletPersistenceService(
      storage: LocalStorageBackend(),
      crypto: crypto,
    );
  }

  static String getStorageKey(String walletId) =>
      WalletPersistenceService.getStorageKey(walletId);

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
    required List<int> accounts,
    required int activeAccount,
    required Set<int> scanningAccounts,
    String? blockHashesJson,
    String? pendingStateJson,
  }) => _defaultInstance.save(
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
    accounts: accounts,
    activeAccount: activeAccount,
    scanningAccounts: scanningAccounts,
    blockHashesJson: blockHashesJson,
    pendingStateJson: pendingStateJson,
  );

  static Future<({String keyHex, String saltHex})?> deriveEncryptionKey(
    String password,
  ) => _defaultInstance.deriveKey(password);

  static Future<SaveWalletResult> saveWithDerivedKey({
    required String walletId,
    required String keyHex,
    required String saltHex,
    required String seed,
    required String network,
    required String? address,
    required String nodeUrl,
    required List<OwnedOutput> outputs,
    required List<WalletTransaction> transactions,
    required int continuousScanCurrentHeight,
    required Set<String> selectedOutputs,
    required List<int> accounts,
    required int activeAccount,
    required Set<int> scanningAccounts,
    String? pendingStateJson,
  }) => _defaultInstance.saveWithDerivedKey(
    walletId: walletId,
    keyHex: keyHex,
    saltHex: saltHex,
    seed: seed,
    network: network,
    address: address,
    nodeUrl: nodeUrl,
    outputs: outputs,
    transactions: transactions,
    continuousScanCurrentHeight: continuousScanCurrentHeight,
    selectedOutputs: selectedOutputs,
    accounts: accounts,
    activeAccount: activeAccount,
    scanningAccounts: scanningAccounts,
    pendingStateJson: pendingStateJson,
  );

  static Future<LoadWalletResult> loadWalletData({
    required String walletId,
    required String password,
  }) => _defaultInstance.load(walletId: walletId, password: password);

  static Future<List<String>> listAvailableWallets() =>
      _defaultInstance.listWalletsAsync();

  static Future<void> clearWalletData(String walletId) =>
      _defaultInstance.clearAsync(walletId);

  static Future<bool> hasWalletData(String walletId) =>
      _defaultInstance.hasAsync(walletId);

  static String extractWalletIdFromFilename(String filename) =>
      WalletSerializer.extractWalletIdFromFilename(filename);

  static Future<ExportWalletResult> exportWallet({
    required String walletId,
  }) async {
    try {
      final rawData = await _defaultInstance.getRawDataAsync(walletId);
      if (rawData == null || rawData.isEmpty) {
        return ExportWalletResult.error('No saved data found for this wallet');
      }

      final now = DateTime.now();
      final timestamp =
          '${now.year}${now.month.toString().padLeft(2, '0')}${now.day.toString().padLeft(2, '0')}-'
          '${now.hour.toString().padLeft(2, '0')}${now.minute.toString().padLeft(2, '0')}${now.second.toString().padLeft(2, '0')}';
      final filename = '${walletId}_$timestamp.monero-wallet';

      final bytes = utf8.encode(rawData);
      final blob = html.Blob([bytes], 'application/octet-stream');

      bool usedSaveAsDialog = false;
      try {
        // The File System Access API gives a real "save as" dialog, but only some
        // browsers have it; the anchor-download path below is the fallback.
        if (_showSaveFilePicker != null) {
          final accept = JSObject()
            ..setProperty(
              'application/octet-stream'.toJS,
              <String>['.monero-wallet'].map((e) => e.toJS).toList().toJS,
            );
          final types = JSObject()
            ..setProperty('description'.toJS, 'Monero Wallet Files'.toJS)
            ..setProperty('accept'.toJS, accept);
          final options = JSObject()
            ..setProperty('suggestedName'.toJS, filename.toJS)
            ..setProperty('types'.toJS, <JSObject>[types].toJS);

          final handle = await (_showSaveFilePicker!.callAsFunction(null, options)!
              as JSPromise<JSObject>)
              .toDart;

          final writable = await (handle.callMethod<JSPromise<JSObject>>(
            'createWritable'.toJS,
          )).toDart;

          await (writable.callMethod<JSPromise<JSAny?>>('write'.toJS, blob as JSAny))
              .toDart;
          await (writable.callMethod<JSPromise<JSAny?>>('close'.toJS)).toDart;

          usedSaveAsDialog = true;
        }
      } catch (e) {
        if (e.toString().contains('aborted')) {
          return ExportWalletResult.cancelled();
        }
      }

      if (!usedSaveAsDialog) {
        final url = html.Url.createObjectUrlFromBlob(blob);
        html.AnchorElement(href: url)
          ..setAttribute('download', filename)
          ..click();
        html.Url.revokeObjectUrl(url);
      }

      return ExportWalletResult.success(
        filename: filename,
        usedSaveAsDialog: usedSaveAsDialog,
      );
    } catch (e) {
      return ExportWalletResult.error('Export failed: $e');
    }
  }

  static Future<ImportWalletResult> importWallet({
    required html.File file,
    required String walletId,
    required String password,
    required bool shouldOverwrite,
  }) async {
    try {
      if (file.size > 10 * 1024 * 1024) {
        return ImportWalletResult.error('File too large (max 10MB)');
      }

      final reader = html.FileReader();
      reader.readAsText(file);
      await reader.onLoad.first;

      final encryptedData = reader.result as String?;
      if (encryptedData == null || encryptedData.isEmpty) {
        return ImportWalletResult.error('File is empty or could not be read');
      }

      final jsonString = await _defaultInstance.decryptRaw(
        password,
        encryptedData,
      );
      if (jsonString == null) {
        return ImportWalletResult.error(
          'Failed to decrypt file (wrong password or corrupted file)',
        );
      }

      jsonDecode(jsonString); // Validate JSON structure

      await _defaultInstance.setRawDataAsync(walletId, encryptedData);

      return ImportWalletResult.success(
        walletId: walletId,
        wasOverwritten: shouldOverwrite,
      );
    } catch (e) {
      return ImportWalletResult.error('Import failed: $e');
    }
  }
}

/// `window.showSaveFilePicker`, absent on browsers without the File System Access API.
@JS('showSaveFilePicker')
external JSFunction? get _showSaveFilePicker;
