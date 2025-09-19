import 'dart:async';
import 'dart:convert';
import 'dart:html' as html;
import 'dart:typed_data';
import 'package:flutter/material.dart';
import '../src/ffi/signal_types.dart';
import '../models/wallet_instance.dart';
import '../services/wallet_persistence_browser.dart';
import '../services/wallet_scan_service.dart';
import '../widgets/password_dialog.dart';
import '../widgets/save_wallet_dialog.dart';
import '../widgets/delete_confirmation_dialog.dart';
import '../widgets/security_warning_dialog.dart';
import 'wallet_state.dart';
import 'output_state.dart';
import 'scan_state.dart';

class FileManagementState extends ChangeNotifier {
  final WalletState _walletState;
  final OutputState _outputState;
  final ScanState _scanState;

  bool isSaving = false;
  bool isLoadingWallet = false;
  String? saveError;
  String? loadError;
  String? lastSaveTime;
  bool isExporting = false;
  bool isImporting = false;
  String? exportError;
  String? importError;
  String? _cachedKeyHex;
  String? _cachedSaltHex;
  Timer? _autoSaveTimer;
  bool autoSaveEnabled = false;

  FileManagementState({
    required WalletState walletState,
    required OutputState outputState,
    required ScanState scanState,
  })  : _walletState = walletState,
        _outputState = outputState,
        _scanState = scanState;

  Future<void> saveWalletData(BuildContext context) async {
    isSaving = true;
    saveError = null;
    notifyListeners();

    final result = await showDialog<Map<String, String>>(
      context: context,
      barrierDismissible: false,
      builder: (context) => SaveWalletDialog(
        initialWalletId: _walletState.walletId.isEmpty ? 'my_wallet' : _walletState.walletId,
        existingWalletIds: _walletState.availableWalletIds,
      ),
    );

    if (result == null) {
      isSaving = false;
      notifyListeners();
      return;
    }

    final walletId = result['walletId']!;
    final password = result['password']!;

    final oldWalletId = _walletState.walletId;
    final isRenamingFromTemp = oldWalletId == 'temp_wallet' && walletId != 'temp_wallet';

    _walletState.walletId = walletId;
    notifyListeners();

    if (password.isEmpty) {
      if (!context.mounted) {
        isSaving = false;
        notifyListeners();
        return;
      }
      final proceed = await SecurityWarningDialog.show(context);
      if (proceed != true) {
        isSaving = false;
        notifyListeners();
        return;
      }
    }

    final activeWallet = _walletState.lifecycle.activeWallet;

    final Set<int> derivedAccounts = {0};
    for (var output in _outputState.filteredOutputs) {
      if (output.subaddressIndex != null) {
        derivedAccounts.add(output.subaddressIndex!.$1);
      }
    }

    final accounts = activeWallet?.accounts ?? derivedAccounts.toList()..sort();
    final activeAccount = activeWallet?.activeAccount ?? _walletState.activeAccount;
    final scanningAccounts = activeWallet?.scanningAccounts ?? derivedAccounts;

    // Request block hashes from Rust before saving
    String? blockHashesJson;
    try {
      final completer = Completer<String?>();
      final sub = BlockHashesResponse.stream.listen((response) {
        if (!completer.isCompleted) {
          completer.complete(response.success ? response.blockHashesJson : null);
        }
      });
      const GetBlockHashesRequest().sendSignalToRust();
      blockHashesJson = await completer.future.timeout(
        const Duration(seconds: 5),
        onTimeout: () => null,
      );
      await sub.cancel();
    } catch (_) {}

    // Request pending state from Rust before saving
    String? pendingStateJson;
    try {
      final completer = Completer<String?>();
      final sub = PendingStateResponse.stream.listen((response) {
        if (!completer.isCompleted) {
          completer.complete(response.success ? response.pendingStateJson : null);
        }
      });
      const GetPendingStateRequest().sendSignalToRust();
      pendingStateJson = await completer.future.timeout(
        const Duration(seconds: 5),
        onTimeout: () => null,
      );
      await sub.cancel();
    } catch (_) {}

    final kim = _outputState.keyImageMap;

    final saveResult = await WalletPersistenceBrowser.saveWalletData(
      walletId: walletId,
      password: password,
      seed: _walletState.seedController.text.trim(),
      network: _walletState.network,
      address: _walletState.derivedAddress,
      nodeUrl: _walletState.nodeUrlController.text,
      outputs: _outputState.filteredOutputs,
      transactions: _outputState.getFilteredTransactions(kim),
      continuousScanCurrentHeight: _walletState.continuousScanCurrentHeight,
      selectedOutputs: const {},
      accounts: accounts,
      activeAccount: activeAccount,
      scanningAccounts: scanningAccounts,
      blockHashesJson: blockHashesJson,
      pendingStateJson: pendingStateJson,
    );

    final success = saveResult.success;

    isSaving = false;
    if (success) {
      saveError = null;
      lastSaveTime = DateTime.now().toString().substring(0, 19);
      _outputState.invalidateStorageBytesCache();
    } else {
      saveError = saveResult.error ?? 'Failed to save wallet data';
    }
    notifyListeners();

    // Derive encryption key for auto-save
    if (success) {
      final derived = await WalletPersistenceBrowser.deriveEncryptionKey(password);
      if (derived != null) {
        _cachedKeyHex = derived.keyHex;
        _cachedSaltHex = derived.saltHex;
        _startAutoSaveTimer();
      }
    }

    if (success) {
      _walletState.refreshAvailableWallets();

      if (isRenamingFromTemp) {
        if (_walletState.lifecycle.openWallets.containsKey('temp_wallet')) {
          final tempWallet = _walletState.lifecycle.openWallets['temp_wallet'];
          _walletState.lifecycle.openWallets.remove('temp_wallet');

          if (tempWallet != null) {
            final renamedWallet = WalletInstance(
              walletId: walletId,
              seed: tempWallet.seed,
              network: tempWallet.network,
              address: tempWallet.address,
              outputs: tempWallet.outputs,
              transactions: tempWallet.transactions,
              currentHeight: tempWallet.currentHeight,
              daemonHeight: tempWallet.daemonHeight,
              isScanning: tempWallet.isScanning,
              isClosed: tempWallet.isClosed,
              activeAccount: tempWallet.activeAccount,
              accounts: tempWallet.accounts,
              outputsByAccount: tempWallet.outputsByAccount,
              scanningAccounts: tempWallet.scanningAccounts,
            );
            _walletState.lifecycle.openWallets[walletId] = renamedWallet;
          }
        }

        if (_walletState.lifecycle.activeWalletId == 'temp_wallet') {
          _walletState.lifecycle.activeWalletId = walletId;
        }

        WalletPersistenceBrowser.clearWalletData('temp_wallet');
      }
    }

    if (success) {
      _walletState.showSnackBar?.call('Wallet "$walletId" saved successfully');
    } else {
      _walletState.showSnackBar?.call('Save failed: $saveError', backgroundColor: Colors.red, seconds: 3);
    }
  }

  Future<void> loadWalletData(BuildContext context, {String? password}) async {
    isLoadingWallet = true;
    loadError = null;
    lastSaveTime = null;
    notifyListeners();

    if (password == null) {
      password = await showDialog<String>(
        context: context,
        barrierDismissible: false,
        builder: (context) => PasswordDialog(
          isUnlock: true,
          title: 'Unlock Wallet Data',
          submitLabel: 'Unlock',
        ),
      );
      if (password == null) {
        isLoadingWallet = false;
        notifyListeners();
        return;
      }
    }

    final loadResult = await WalletPersistenceBrowser.loadWalletData(
      walletId: _walletState.walletId,
      password: password,
    );

    if (!loadResult.success) {
      isLoadingWallet = false;
      loadError = loadResult.error ?? 'Failed to load wallet data';
      notifyListeners();
      return;
    }

    final seed = loadResult.seed!;
    final network = loadResult.network!;
    final address = loadResult.address;
    final loadedOutputs = loadResult.outputs!;
    final loadedTransactions = loadResult.transactions!;
    final loadedHeight = loadResult.continuousScanCurrentHeight!;
    final loadedSelectedOutputs = loadResult.selectedOutputs!;
    final loadedAccounts = loadResult.accounts ?? [0];
    final loadedOutputsByAccount = loadResult.outputsByAccount
        ?? _walletState.reconstructOutputsByAccount(loadedOutputs);

    _walletState.isRestoringWallet = true;

    _walletState.seedController.text = seed;
    _walletState.network = network;
    _walletState.derivedAddress = address;
    _walletState.nodeUrlController.text = loadResult.nodeUrl!;
    _walletState.continuousScanCurrentHeight = loadedHeight;
    _scanState.continuousScanTargetHeight = 0;
    _scanState.isSynced = false;
    _scanState.daemonHeight = loadedHeight > 0 ? loadedHeight : null;
    _scanState.isContinuousScanning = false;
    _scanState.isContinuousPaused = loadedHeight > 0;

    if (loadedHeight > 0) {
      _walletState.blockHeightController.text = loadedHeight.toString();
      _walletState.blockHeightUserEdited = false;
    }

    isLoadingWallet = false;
    loadError = null;
    _outputState.invalidateStorageBytesCache();

    final resolvedAddress = address ?? _walletState.derivedAddress ?? '';
    if (seed.isNotEmpty && resolvedAddress.isNotEmpty) {
      _walletState.openWallet(
        _walletState.walletId,
        seed,
        network,
        resolvedAddress,
        accounts: loadedAccounts,
        outputsByAccount: loadedOutputsByAccount,
        activeAccount: 0,
      );
    }

    _walletState.isRestoringWallet = false;

    _walletState.lifecycle.restoreLoadedData(
      outputs: loadedOutputs,
      transactions: loadedTransactions,
      selectedOutputs: loadedSelectedOutputs,
      scanHeight: loadedHeight,
      daemonHeight: _scanState.daemonHeight ?? 0,
    );
    _walletState.notify();

    notifyListeners();

    RestoreWalletDataRequest(
      seed: seed,
      network: network,
      outputs: loadedOutputs,
      daemonHeight: _scanState.daemonHeight ?? 0,
      currentHeight: loadedHeight,
      blockHashesJson: loadResult.blockHashesJson,
      pendingStateJson: loadResult.pendingStateJson,
      passphrase: _walletState.passphrase,
      bip39AccountIndex: _walletState.bip39AccountIndex,
    ).sendSignalToRust();

    // Rebuild Dart-side pending key images from restored pending state
    _walletState.pendingSpentKeyImages.clear();
    if (loadResult.pendingStateJson != null) {
      try {
        final blob = jsonDecode(loadResult.pendingStateJson!) as Map<String, dynamic>;
        final pendingSpends = blob['pending_spends'] as Map<String, dynamic>? ?? {};
        _walletState.pendingSpentKeyImages.addAll(pendingSpends.keys);
      } catch (_) {}
    }

    _walletState.deriveAddress();

    // Cache encryption key so auto-save can work without a manual save
    final derived = await WalletPersistenceBrowser.deriveEncryptionKey(password);
    if (derived != null) {
      _cachedKeyHex = derived.keyHex;
      _cachedSaltHex = derived.saltHex;
      _startAutoSaveTimer();
    }

    _walletState.showSnackBar?.call('Wallet data loaded successfully');
  }

  Future<void> exportWallet(BuildContext context) async {
    final seed = _walletState.seedController.text.trim();
    if (seed.isEmpty) {
      exportError = 'No seed available for export';
      notifyListeners();
      _walletState.showSnackBar?.call('No seed to export', backgroundColor: Colors.orange);
      return;
    }

    if (!context.mounted) return;
    final password = await showDialog<String>(
      context: context,
      barrierDismissible: false,
      builder: (context) => PasswordDialog(
        isUnlock: false,
        title: '.keys File Password',
        submitLabel: 'Export',
      ),
    );

    if (password == null) return;

    isExporting = true;
    exportError = null;
    notifyListeners();

    try {
      final completer = Completer<ExportKeysFileResponse>();
      final sub = ExportKeysFileResponse.stream.listen((response) {
        if (!completer.isCompleted) completer.complete(response);
      });

      final network = _walletState.network.isNotEmpty ? _walletState.network : 'mainnet';

      ExportKeysFileRequest(
        seed: seed,
        network: network,
        password: password,
      ).sendSignalToRust();

      final response = await completer.future.timeout(
        const Duration(seconds: 30),
        onTimeout: () => ExportKeysFileResponse(
          success: false,
          error: 'Timeout waiting for encryption',
        ),
      );
      await sub.cancel();

      if (!response.success || response.fileBytesHex == null) {
        isExporting = false;
        exportError = response.error ?? 'Export failed';
        notifyListeners();
        _walletState.showSnackBar?.call(
          'Export failed: ${response.error}',
          backgroundColor: Colors.red,
          seconds: 4,
        );
        return;
      }

      final fileBytes = _hexDecode(response.fileBytesHex!);
      final blob = html.Blob([fileBytes], 'application/octet-stream');
      final url = html.Url.createObjectUrlFromBlob(blob);
      final walletName = _walletState.walletId.isNotEmpty ? _walletState.walletId : 'wallet';
      final anchor = html.AnchorElement(href: url)
        ..setAttribute('download', '$walletName.keys')
        ..click();
      html.Url.revokeObjectUrl(url);

      isExporting = false;
      exportError = null;
      notifyListeners();

      _walletState.showSnackBar?.call('Exported $walletName.keys', seconds: 3);
    } catch (e) {
      isExporting = false;
      exportError = 'Export failed: $e';
      notifyListeners();
      _walletState.showSnackBar?.call('Export failed: $e', backgroundColor: Colors.red, seconds: 4);
    }
  }

  static List<int> _hexDecode(String hex) {
    final result = <int>[];
    for (var i = 0; i < hex.length; i += 2) {
      result.add(int.parse(hex.substring(i, i + 2), radix: 16));
    }
    return result;
  }

  Future<void> importWallet(BuildContext context) async {
    importError = null;
    notifyListeners();

    try {
      final uploadInput = html.FileUploadInputElement();
      uploadInput.accept = '.keys,*';
      uploadInput.click();

      try {
        await uploadInput.onChange.first.timeout(const Duration(seconds: 120));
      } on TimeoutException {
        return;
      }

      final files = uploadInput.files;
      if (files == null || files.isEmpty) return;

      isImporting = true;
      notifyListeners();

      final file = files[0];

      if (!context.mounted) return;
      final password = await showDialog<String>(
        context: context,
        barrierDismissible: false,
        builder: (context) => PasswordDialog(
          isUnlock: true,
          title: '.keys File Password',
          submitLabel: 'Import',
        ),
      );

      if (password == null) {
        isImporting = false;
        notifyListeners();
        return;
      }

      final reader = html.FileReader();
      reader.readAsArrayBuffer(file);
      await reader.onLoadEnd.first;
      final result = reader.result;
      if (result == null) {
        isImporting = false;
        importError = 'Failed to read file';
        notifyListeners();
        return;
      }
      final bytes = Uint8List.view(result as ByteBuffer);
      final hexBytes = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();

      final completer = Completer<ImportKeysFileResponse>();
      final sub = ImportKeysFileResponse.stream.listen((response) {
        if (!completer.isCompleted) completer.complete(response);
      });

      ImportKeysFileRequest(
        fileBytesHex: hexBytes,
        password: password,
      ).sendSignalToRust();

      final response = await completer.future.timeout(
        const Duration(seconds: 30),
        onTimeout: () => ImportKeysFileResponse(
          success: false,
          error: 'Timeout waiting for decryption',
          creationTimestamp: 0,
          watchOnly: false,
        ),
      );
      await sub.cancel();

      if (!response.success) {
        isImporting = false;
        importError = response.error ?? 'Failed to decrypt .keys file';
        notifyListeners();
        _walletState.showSnackBar?.call(
          'Import failed: ${response.error}',
          backgroundColor: Colors.red,
          seconds: 4,
        );
        return;
      }

      final mnemonic = response.mnemonic;
      if (mnemonic == null || mnemonic.isEmpty) {
        isImporting = false;
        importError = response.watchOnly
            ? 'View-only wallet import not yet supported'
            : 'No mnemonic recovered from .keys file';
        notifyListeners();
        _walletState.showSnackBar?.call(importError!, backgroundColor: Colors.orange, seconds: 4);
        return;
      }

      if (_scanState.isContinuousScanning) {
        WalletScanService.pauseContinuousScan();
        _scanState.isContinuousScanning = false;
        _scanState.isContinuousPaused = false;
      }

      final walletName = file.name.replaceAll('.keys', '');
      final network = _walletState.network.isNotEmpty ? _walletState.network : 'stagenet';

      _walletState.seedController.text = mnemonic;
      _walletState.network = network;
      _walletState.walletId = walletName.isNotEmpty ? walletName : 'imported';
      _walletState.deriveAddress();

      isImporting = false;
      importError = null;
      notifyListeners();

      _walletState.showSnackBar?.call(
        'Imported ${response.watchOnly ? "view-only " : ""}wallet from ${file.name}',
        seconds: 3,
      );
    } catch (e) {
      isImporting = false;
      importError = 'Import failed: $e';
      notifyListeners();
      _walletState.showSnackBar?.call('Import failed: $e', backgroundColor: Colors.red, seconds: 4);
    }
  }

  Future<void> clearStoredData(BuildContext context) async {
    final confirmed = await DeleteConfirmationDialog.show(context, _walletState.walletId);
    if (confirmed != true || !context.mounted) return;

    final deletedWalletId = _walletState.walletId;
    WalletPersistenceBrowser.clearWalletData(deletedWalletId);
    _outputState.invalidateStorageBytesCache();
    _walletState.refreshAvailableWallets();
    _walletState.startNewWallet();
    _walletState.showSnackBar?.call('Deleted wallet: $deletedWalletId');
  }

  void setAutoSaveEnabled(bool enabled) {
    autoSaveEnabled = enabled;
    if (enabled && _cachedKeyHex != null) {
      _startAutoSaveTimer();
    } else if (!enabled) {
      _autoSaveTimer?.cancel();
    }
    notifyListeners();
  }

  void _startAutoSaveTimer() {
    _autoSaveTimer?.cancel();
    if (!autoSaveEnabled) return;
    _autoSaveTimer = Timer.periodic(const Duration(minutes: 2), (_) {
      autoSaveIfReady();
    });
  }

  bool _isAutoSaving = false;

  Future<void> autoSaveIfReady() async {
    if (!autoSaveEnabled) return;
    if (_cachedKeyHex == null || _walletState.walletId.isEmpty || _walletState.walletId == 'temp_wallet') return;
    if (_scanState.isContinuousScanning && !_scanState.isSynced) return;
    if (isSaving || _isAutoSaving) return;

    _isAutoSaving = true;
    try {
      final activeWallet = _walletState.lifecycle.activeWallet;
      final accounts = activeWallet?.accounts ?? [0];
      final activeAccount = activeWallet?.activeAccount ?? _walletState.activeAccount;
      final scanningAccounts = activeWallet?.scanningAccounts ?? {0};

      final saveResult = await WalletPersistenceBrowser.saveWithDerivedKey(
        walletId: _walletState.walletId,
        keyHex: _cachedKeyHex!,
        saltHex: _cachedSaltHex!,
        seed: _walletState.seedController.text.trim(),
        network: _walletState.network,
        address: _walletState.derivedAddress,
        nodeUrl: _walletState.nodeUrlController.text,
        outputs: _walletState.allOutputs,
        transactions: _walletState.allTransactions,
        continuousScanCurrentHeight: _walletState.continuousScanCurrentHeight,
        selectedOutputs: const {},
        accounts: accounts,
        activeAccount: activeAccount,
        scanningAccounts: scanningAccounts,
      );

      if (saveResult.success) {
        lastSaveTime = DateTime.now().toString().substring(0, 19);
        _outputState.invalidateStorageBytesCache();
        notifyListeners();
      }
    } finally {
      _isAutoSaving = false;
    }
  }

  void cancelAutoSave() {
    _autoSaveTimer?.cancel();
    _cachedKeyHex = null;
    _cachedSaltHex = null;
    autoSaveEnabled = false;
  }

  @override
  void dispose() {
    _autoSaveTimer?.cancel();
    super.dispose();
  }
}
