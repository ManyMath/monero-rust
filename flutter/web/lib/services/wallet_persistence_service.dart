import 'dart:convert';
import '../src/bindings/bindings.dart';
import '../models/wallet_instance.dart';
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
    required List<int> accounts,
    required int activeAccount,
    required Set<int> scanningAccounts,
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
        accounts: accounts,
        activeAccount: activeAccount,
        scanningAccounts: scanningAccounts,
      );

      final jsonString = jsonEncode(walletData);

      final encryptedData = await _crypto.encrypt(password, jsonString);
      if (encryptedData == null) {
        return SaveWalletResult.error('Encryption failed');
      }

      _storage.set(storageKey, encryptedData);
      return SaveWalletResult.success();
    } catch (e) {
      return SaveWalletResult.error('Save failed: $e');
    }
  }

  Future<LoadWalletResult> load({
    required String walletId,
    required String password,
  }) async {
    try {
      final storageKey = getStorageKey(walletId);

      final encryptedData = _storage.get(storageKey);
      if (encryptedData == null) {
        return LoadWalletResult.error(
            'No stored wallet data found for wallet: $walletId');
      }

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
        accounts: parsed.accounts,
        outputsByAccount: parsed.outputsByAccount,
        activeAccount: parsed.activeAccount,
        scanningAccounts: parsed.scanningAccounts,
      );
    } catch (e) {
      return LoadWalletResult.error('Failed to parse wallet data: $e');
    }
  }

  List<String> listWallets() {
    final walletIds = <String>[];
    for (final key in _storage.keys) {
      if (key.startsWith('monero_wallet_')) {
        walletIds.add(key.substring('monero_wallet_'.length));
      }
    }
    walletIds.sort();
    return walletIds;
  }

  void clear(String walletId) {
    final storageKey = getStorageKey(walletId);
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

  /// Save a WalletInstance directly (convenience method)
  /// This uses WalletInstance.toJson() to serialize the wallet state
  Future<SaveWalletResult> saveWalletInstance({
    required WalletInstance wallet,
    required String password,
    required String nodeUrl,
    required List<WalletTransaction> transactions,
    required Set<String> selectedOutputs,
  }) async {
    return save(
      walletId: wallet.walletId,
      password: password,
      seed: wallet.seed,
      network: wallet.network,
      address: wallet.address,
      nodeUrl: nodeUrl,
      outputs: wallet.outputs,
      transactions: transactions,
      continuousScanCurrentHeight: wallet.currentHeight,
      selectedOutputs: selectedOutputs,
      accounts: wallet.accounts,
      activeAccount: wallet.activeAccount,
      scanningAccounts: wallet.scanningAccounts,
    );
  }

  /// Load wallet data and create a WalletInstance (convenience method)
  /// This uses WalletInstance.fromJson() to deserialize the wallet state
  Future<LoadWalletInstanceResult> loadWalletInstance({
    required String walletId,
    required String password,
  }) async {
    final result = await load(walletId: walletId, password: password);

    if (!result.success) {
      return LoadWalletInstanceResult.error(result.error ?? 'Unknown error');
    }

    try {
      final wallet = WalletInstance(
        walletId: walletId,
        seed: result.seed!,
        network: result.network!,
        address: result.address ?? '',
        outputs: result.outputs!,
        currentHeight: result.continuousScanCurrentHeight!,
        daemonHeight: result.continuousScanCurrentHeight!,
        isScanning: false,
        isClosed: false,
        activeAccount: result.activeAccount!,
        accounts: result.accounts!,
        outputsByAccount: result.outputsByAccount!,
        scanningAccounts: result.scanningAccounts!,
      );

      return LoadWalletInstanceResult.success(
        wallet: wallet,
        nodeUrl: result.nodeUrl!,
        transactions: result.transactions!,
        selectedOutputs: result.selectedOutputs!,
      );
    } catch (e) {
      return LoadWalletInstanceResult.error('Failed to create wallet instance: $e');
    }
  }
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
  final List<int>? accounts;
  final Map<int, List<OwnedOutput>>? outputsByAccount;
  final int? activeAccount;
  final Set<int>? scanningAccounts;

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
    this.accounts,
    this.outputsByAccount,
    this.activeAccount,
    this.scanningAccounts,
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
    required List<int> accounts,
    required Map<int, List<OwnedOutput>> outputsByAccount,
    required int activeAccount,
    required Set<int> scanningAccounts,
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
        accounts: accounts,
        outputsByAccount: outputsByAccount,
        activeAccount: activeAccount,
        scanningAccounts: scanningAccounts,
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

class LoadWalletInstanceResult {
  final bool success;
  final String? error;
  final WalletInstance? wallet;
  final String? nodeUrl;
  final List<WalletTransaction>? transactions;
  final Set<String>? selectedOutputs;

  LoadWalletInstanceResult._({
    required this.success,
    this.error,
    this.wallet,
    this.nodeUrl,
    this.transactions,
    this.selectedOutputs,
  });

  factory LoadWalletInstanceResult.success({
    required WalletInstance wallet,
    required String nodeUrl,
    required List<WalletTransaction> transactions,
    required Set<String> selectedOutputs,
  }) =>
      LoadWalletInstanceResult._(
        success: true,
        wallet: wallet,
        nodeUrl: nodeUrl,
        transactions: transactions,
        selectedOutputs: selectedOutputs,
      );

  factory LoadWalletInstanceResult.error(String error) =>
      LoadWalletInstanceResult._(success: false, error: error);
}
