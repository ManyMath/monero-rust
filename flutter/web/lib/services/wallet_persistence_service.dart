import 'dart:async';
import 'dart:convert';
import 'dart:html' as html;
import 'dart:js_util' as js_util;
import 'package:flutter/foundation.dart';
import 'package:tuple/tuple.dart';
import '../src/bindings/bindings.dart';
import '../models/wallet_transaction.dart';

/// Service for handling wallet data persistence operations
/// including save, load, import, export, and localStorage management.
class WalletPersistenceService {
  /// Get the localStorage key for a specific wallet ID
  static String getStorageKey(String walletId) => 'monero_wallet_$walletId';

  /// Save wallet data to localStorage with encryption
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
  }) async {
    try {
      final storageKey = getStorageKey(walletId);

      // Serialize wallet state to JSON
      final walletData = {
        'seed': seed,
        'network': network,
        'address': address,
        'nodeUrl': nodeUrl,
        'outputs': outputs.map((o) => {
          'txHash': o.txHash,
          'outputIndex': o.outputIndex,
          'amount': o.amount.toString(),
          'amountXmr': o.amountXmr,
          'key': o.key,
          'keyOffset': o.keyOffset,
          'commitmentMask': o.commitmentMask,
          'subaddressIndex': o.subaddressIndex != null
              ? [o.subaddressIndex!.item1, o.subaddressIndex!.item2]
              : null,
          'paymentId': o.paymentId,
          'receivedOutputBytes': o.receivedOutputBytes,
          'blockHeight': o.blockHeight.toString(),
          'spent': o.spent,
          'keyImage': o.keyImage,
        }).toList(),
        'transactions': transactions.map((t) => t.toJson()).toList(),
        'scanState': {
          'continuousScanCurrentHeight': continuousScanCurrentHeight,
        },
        'selectedOutputs': selectedOutputs.toList(),
      };

      final jsonString = jsonEncode(walletData);
      debugPrint('[SAVE] Serialized ${outputs.length} outputs, ${transactions.length} transactions');

      // Wait for encrypted response from Rust
      final completer = Completer<String?>();
      final subscription = WalletDataSavedResponse.rustSignalStream.listen((signal) {
        if (!completer.isCompleted) {
          if (signal.message.success && signal.message.encryptedData != null) {
            completer.complete(signal.message.encryptedData);
          } else {
            completer.complete(null);
          }
        }
      });

      // Send save request to Rust for encryption
      SaveWalletDataRequest(
        password: password,
        walletDataJson: jsonString,
      ).sendSignalToRust();

      final encryptedData = await completer.future.timeout(
        const Duration(seconds: 10),
        onTimeout: () => null,
      );

      await subscription.cancel();

      if (encryptedData == null) {
        return SaveWalletResult.error('Encryption failed');
      }

      // Store encrypted data in localStorage
      html.window.localStorage[storageKey] = encryptedData;
      debugPrint('[SAVE] Stored to localStorage key: $storageKey');

      return SaveWalletResult.success();
    } catch (e) {
      debugPrint('[SAVE] Error: $e');
      return SaveWalletResult.error('Save failed: $e');
    }
  }

  /// Load wallet data from localStorage with decryption
  static Future<LoadWalletResult> loadWalletData({
    required String walletId,
    required String password,
  }) async {
    try {
      final storageKey = getStorageKey(walletId);
      debugPrint('[LOAD] Looking for wallet data at key: $storageKey');

      final encryptedData = html.window.localStorage[storageKey];
      if (encryptedData == null) {
        return LoadWalletResult.error('No stored wallet data found for wallet: $walletId');
      }
      debugPrint('[LOAD] Found encrypted data (${encryptedData.length} chars)');

      // Wait for decrypted response from Rust
      final completer = Completer<String?>();
      final subscription = WalletDataLoadedResponse.rustSignalStream.listen((signal) {
        if (!completer.isCompleted) {
          if (signal.message.success && signal.message.walletDataJson != null) {
            completer.complete(signal.message.walletDataJson);
          } else {
            completer.complete(null);
          }
        }
      });

      // Send load request to Rust for decryption
      LoadWalletDataRequest(
        password: password,
        encryptedData: encryptedData,
      ).sendSignalToRust();

      final jsonString = await completer.future.timeout(
        const Duration(seconds: 10),
        onTimeout: () => null,
      );

      await subscription.cancel();

      if (jsonString == null) {
        return LoadWalletResult.error('Failed to decrypt wallet data (wrong password?)');
      }

      // Parse wallet data
      final walletData = jsonDecode(jsonString) as Map<String, dynamic>;

      // Restore outputs
      final outputs = (walletData['outputs'] as List).map((o) {
        final outputData = o as Map<String, dynamic>;
        return OwnedOutput(
          txHash: outputData['txHash'] as String,
          outputIndex: outputData['outputIndex'] as int,
          amount: Uint64(BigInt.parse(outputData['amount'] as String)),
          amountXmr: outputData['amountXmr'] as String,
          key: outputData['key'] as String,
          keyOffset: outputData['keyOffset'] as String,
          commitmentMask: outputData['commitmentMask'] as String,
          subaddressIndex: outputData['subaddressIndex'] != null
              ? Tuple2<int, int>(
                  outputData['subaddressIndex'][0] as int,
                  outputData['subaddressIndex'][1] as int,
                )
              : null,
          paymentId: outputData['paymentId'] as String?,
          receivedOutputBytes: outputData['receivedOutputBytes'] as String,
          blockHeight: Uint64(BigInt.parse(outputData['blockHeight'] as String)),
          spent: outputData['spent'] as bool,
          keyImage: outputData['keyImage'] as String,
        );
      }).toList();

      // Restore transactions
      final transactions = walletData['transactions'] != null
          ? (walletData['transactions'] as List)
              .map((t) => WalletTransaction.fromJson(t as Map<String, dynamic>))
              .toList()
          : <WalletTransaction>[];

      final scanState = walletData['scanState'] as Map<String, dynamic>;
      final continuousScanCurrentHeight = scanState['continuousScanCurrentHeight'] as int;

      // Restore selected outputs
      final selectedOutputs = Set<String>.from(walletData['selectedOutputs'] as List);

      return LoadWalletResult.success(
        seed: walletData['seed'] as String? ?? '',
        network: walletData['network'] as String? ?? 'stagenet',
        address: walletData['address'] as String?,
        nodeUrl: walletData['nodeUrl'] as String? ?? 'http://127.0.0.1:38081',
        outputs: outputs,
        transactions: transactions,
        continuousScanCurrentHeight: continuousScanCurrentHeight,
        selectedOutputs: selectedOutputs,
      );
    } catch (e) {
      debugPrint('[LOAD] Error: $e');
      return LoadWalletResult.error('Failed to parse wallet data: $e');
    }
  }

  /// Export wallet data as an encrypted file
  static Future<ExportWalletResult> exportWallet({
    required String walletId,
  }) async {
    try {
      final storageKey = getStorageKey(walletId);

      // Validation - check if wallet has saved data
      if (!html.window.localStorage.containsKey(storageKey)) {
        return ExportWalletResult.error('No saved data found for this wallet');
      }

      // Get encrypted data from localStorage
      final encryptedData = html.window.localStorage[storageKey];
      if (encryptedData == null || encryptedData.isEmpty) {
        return ExportWalletResult.error('Wallet data is empty');
      }

      // Generate filename with timestamp
      final now = DateTime.now();
      final timestamp = '${now.year}${now.month.toString().padLeft(2, '0')}${now.day.toString().padLeft(2, '0')}-'
          '${now.hour.toString().padLeft(2, '0')}${now.minute.toString().padLeft(2, '0')}${now.second.toString().padLeft(2, '0')}';
      final filename = '${walletId}_$timestamp.monero-wallet';

      // Create blob with wallet data
      final bytes = utf8.encode(encryptedData);
      final blob = html.Blob([bytes], 'application/octet-stream');

      // Try to use File System Access API for "Save As" dialog (Chrome 86+, Edge 86+)
      // Falls back to automatic download for unsupported browsers (Firefox, Safari)
      bool usedSaveAsDialog = false;
      try {
        if (js_util.hasProperty(html.window, 'showSaveFilePicker')) {
          debugPrint('[EXPORT] Using File System Access API (Save As dialog)');

          // Configure file picker options
          final options = js_util.newObject();
          js_util.setProperty(options, 'suggestedName', filename);

          // Set file types filter
          final types = js_util.newObject();
          js_util.setProperty(types, 'description', 'Monero Wallet Files');
          final accept = js_util.newObject();
          js_util.setProperty(accept, 'application/octet-stream', ['.monero-wallet']);
          js_util.setProperty(types, 'accept', accept);
          js_util.setProperty(options, 'types', [types]);

          // Show save file picker
          final fileHandlePromise = js_util.callMethod(
            html.window,
            'showSaveFilePicker',
            [options],
          );
          final fileHandle = await js_util.promiseToFuture(fileHandlePromise);

          // Create writable stream
          final writablePromise = js_util.callMethod(fileHandle, 'createWritable', []);
          final writable = await js_util.promiseToFuture(writablePromise);

          // Write blob to file
          final writePromise = js_util.callMethod(writable, 'write', [blob]);
          await js_util.promiseToFuture(writePromise);

          // Close the file
          final closePromise = js_util.callMethod(writable, 'close', []);
          await js_util.promiseToFuture(closePromise);

          usedSaveAsDialog = true;
          debugPrint('[EXPORT] File saved via Save As dialog');
        }
      } catch (e) {
        // User cancelled the save dialog or API not supported
        if (e.toString().contains('aborted')) {
          debugPrint('[EXPORT] User cancelled save dialog');
          return ExportWalletResult.cancelled();
        }
        debugPrint('[EXPORT] File System Access API not available or failed: $e');
        // Continue to fallback method
      }

      // Fallback: Use traditional download method (Firefox, Safari, or if API failed)
      if (!usedSaveAsDialog) {
        debugPrint('[EXPORT] Using fallback download method');
        final url = html.Url.createObjectUrlFromBlob(blob);
        final anchor = html.AnchorElement(href: url)
          ..setAttribute('download', filename)
          ..click();
        html.Url.revokeObjectUrl(url);
      }

      debugPrint('[EXPORT] Successfully exported wallet: $walletId (method: ${usedSaveAsDialog ? 'Save As dialog' : 'auto-download'})');
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
      debugPrint('[IMPORT] Selected file: ${file.name} (${file.size} bytes)');

      // Validate file size (max 10MB as sanity check)
      if (file.size > 10 * 1024 * 1024) {
        return ImportWalletResult.error('File too large (max 10MB)');
      }

      // Read file content (WASM-compatible)
      final reader = html.FileReader();
      reader.readAsText(file);
      await reader.onLoad.first;

      final encryptedData = reader.result as String?;
      if (encryptedData == null || encryptedData.isEmpty) {
        return ImportWalletResult.error('File is empty or could not be read');
      }

      debugPrint('[IMPORT] Read ${encryptedData.length} characters from file');

      // Verify decryption by attempting to load
      final completer = Completer<String?>();
      final subscription = WalletDataLoadedResponse.rustSignalStream.listen((signal) {
        if (!completer.isCompleted) {
          if (signal.message.success && signal.message.walletDataJson != null) {
            completer.complete(signal.message.walletDataJson);
          } else {
            completer.complete(null);
          }
        }
      });

      LoadWalletDataRequest(
        password: password,
        encryptedData: encryptedData,
      ).sendSignalToRust();

      final jsonString = await completer.future.timeout(
        const Duration(seconds: 10),
        onTimeout: () => null,
      );

      await subscription.cancel();

      if (jsonString == null) {
        return ImportWalletResult.error('Failed to decrypt file (wrong password or corrupted file)');
      }

      // Verify JSON is valid
      jsonDecode(jsonString);

      // Store to localStorage with new wallet ID
      final storageKey = getStorageKey(walletId);
      html.window.localStorage[storageKey] = encryptedData;
      debugPrint('[IMPORT] Stored wallet data to: $storageKey');

      debugPrint('[IMPORT] Successfully ${shouldOverwrite ? 'overwritten' : 'imported'} wallet: $walletId');
      return ImportWalletResult.success(
        walletId: walletId,
        wasOverwritten: shouldOverwrite,
      );
    } catch (e) {
      debugPrint('[IMPORT] Import failed: $e');
      return ImportWalletResult.error('Import failed: $e');
    }
  }

  /// Scan localStorage for all available wallet IDs
  static List<String> listAvailableWallets() {
    debugPrint('[WALLET] Scanning localStorage for available wallets...');
    final walletIds = <String>[];

    // Scan all localStorage keys - only include wallets that have saved data
    for (var i = 0; i < html.window.localStorage.length; i++) {
      final key = html.window.localStorage.keys.elementAt(i);
      if (key.startsWith('monero_wallet_')) {
        final walletId = key.substring('monero_wallet_'.length);
        walletIds.add(walletId);
      }
    }

    walletIds.sort();
    debugPrint('[WALLET] Found ${walletIds.length} wallets: ${walletIds.join(', ')}');
    return walletIds;
  }

  /// Clear/delete wallet data from localStorage
  static void clearWalletData(String walletId) {
    final storageKey = getStorageKey(walletId);
    debugPrint('[STORAGE] Clearing data for wallet: $walletId');
    html.window.localStorage.remove(storageKey);
  }

  /// Check if wallet data exists in localStorage
  static bool hasWalletData(String walletId) {
    final storageKey = getStorageKey(walletId);
    return html.window.localStorage.containsKey(storageKey);
  }

  /// Extract suggested wallet ID from filename
  static String extractWalletIdFromFilename(String filename) {
    String suggestedWalletId = filename;

    // Remove .monero-wallet extension
    if (suggestedWalletId.endsWith('.monero-wallet')) {
      suggestedWalletId = suggestedWalletId.substring(0, suggestedWalletId.length - 14);
    }

    // Remove timestamp if present (pattern: _20260204-143025)
    final timestampRegex = RegExp(r'_\d{8}-\d{6}$');
    suggestedWalletId = suggestedWalletId.replaceAll(timestampRegex, '');

    // Ensure valid wallet ID
    if (suggestedWalletId.isEmpty || !RegExp(r'^[a-zA-Z0-9_-]+$').hasMatch(suggestedWalletId)) {
      suggestedWalletId = 'imported_wallet';
    }

    return suggestedWalletId;
  }
}

/// Result type for save wallet operation
class SaveWalletResult {
  final bool success;
  final String? error;

  SaveWalletResult._({
    required this.success,
    this.error,
  });

  factory SaveWalletResult.success() {
    return SaveWalletResult._(success: true);
  }

  factory SaveWalletResult.error(String error) {
    return SaveWalletResult._(
      success: false,
      error: error,
    );
  }
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
  }) {
    return LoadWalletResult._(
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
  }

  factory LoadWalletResult.error(String error) {
    return LoadWalletResult._(
      success: false,
      error: error,
    );
  }
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
  }) {
    return ExportWalletResult._(
      success: true,
      filename: filename,
      usedSaveAsDialog: usedSaveAsDialog,
    );
  }

  factory ExportWalletResult.cancelled() {
    return ExportWalletResult._(
      success: false,
      cancelled: true,
    );
  }

  factory ExportWalletResult.error(String error) {
    return ExportWalletResult._(
      success: false,
      error: error,
    );
  }
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
  }) {
    return ImportWalletResult._(
      success: true,
      walletId: walletId,
      wasOverwritten: wasOverwritten,
    );
  }

  factory ImportWalletResult.error(String error) {
    return ImportWalletResult._(
      success: false,
      error: error,
    );
  }
}
