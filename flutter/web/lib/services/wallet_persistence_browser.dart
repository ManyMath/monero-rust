import 'dart:convert';
import 'dart:html' as html;
import 'dart:js_util' as js_util;
import '../src/ffi/signal_types.dart';
import '../models/wallet_transaction.dart';
import 'chrome_storage_backend.dart';
import 'wallet_persistence_service.dart';
import 'local_storage_backend.dart';
import 'migrating_storage_backend.dart';
import 'rust_crypto_backend.dart';
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
      return WalletPersistenceService.async(
        storage: MigratingStorageBackend(
          primary: ChromeStorageBackend(),
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
        if (js_util.hasProperty(html.window, 'showSaveFilePicker')) {
          final options = js_util.newObject();
          js_util.setProperty(options, 'suggestedName', filename);

          final types = js_util.newObject();
          js_util.setProperty(types, 'description', 'Monero Wallet Files');
          final accept = js_util.newObject();
          js_util.setProperty(accept, 'application/octet-stream', [
            '.monero-wallet',
          ]);
          js_util.setProperty(types, 'accept', accept);
          js_util.setProperty(options, 'types', [types]);

          final fileHandlePromise = js_util.callMethod(
            html.window,
            'showSaveFilePicker',
            [options],
          );
          final fileHandle = await js_util.promiseToFuture(fileHandlePromise);

          final writablePromise = js_util.callMethod(
            fileHandle,
            'createWritable',
            [],
          );
          final writable = await js_util.promiseToFuture(writablePromise);

          final writePromise = js_util.callMethod(writable, 'write', [blob]);
          await js_util.promiseToFuture(writePromise);

          final closePromise = js_util.callMethod(writable, 'close', []);
          await js_util.promiseToFuture(closePromise);

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
