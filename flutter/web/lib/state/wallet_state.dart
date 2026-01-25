import 'dart:async';
import 'package:flutter/material.dart';
import 'package:monero_extension/utils/network_utils.dart';
import '../src/ffi/signal_types.dart';
import '../src/logging.dart';
import '../utils/key_parser.dart';
import '../models/wallet_instance.dart';
import '../models/wallet_transaction.dart';
import '../services/wallet_lifecycle_manager.dart';
import '../widgets/close_wallet_dialog.dart';
import '../src/ffi/signal_hub.dart';

typedef HasStoredWallet = Future<bool> Function(String walletId);
typedef ClearStoredWallet = Future<void> Function(String walletId);

class WalletState extends ChangeNotifier {
  static const _tag = 'WalletState';
  final WalletLifecycleManager lifecycle;
  final SignalHub _signalHub;
  final HasStoredWallet? _hasStoredWallet;
  final ClearStoredWallet? _clearStoredWallet;

  final seedController = TextEditingController();
  final passphraseController = TextEditingController();
  final viewKeyController = TextEditingController();
  final spendKeyController = TextEditingController();
  final nodeUrlController = TextEditingController(
    text: 'http://127.0.0.1:38081',
  );
  final blockHeightController = TextEditingController();
  final blockHeightFocusNode = FocusNode();
  bool blockHeightUserEdited = false;

  String network = 'stagenet';
  String seedType = '25 word (classic)';
  String passphrase = '';
  int bip39AccountIndex = 0;
  String? validationError;
  String? derivedAddress;
  String? responseError;
  String? secretSpendKey;
  String? secretViewKey;
  String? publicSpendKey;
  String? publicViewKey;
  int? polyseedRestoreHeight;
  String? derivedLegacySeed;
  Timer? _debounceTimer;
  String? _pendingImportedWalletId;
  String? _pendingImportedWalletSeed;
  String? _pendingImportedSpendSecretKey;
  String? _pendingImportedViewSecretKey;

  bool isChangingSeed = false;
  bool isRestoringWallet = false;
  VoidCallback? onScanStopped;

  final Map<String, String> subaddresses = {};
  final Set<String> _pendingSubaddresses = {};
  final Set<String> pendingSpentKeyImages = {};

  void Function(String, {Color? backgroundColor, int seconds})? showSnackBar;
  bool Function()? checkIsContinuousScanning;
  VoidCallback? onWalletStateCleared;
  void Function(int? daemonHeight)? onDaemonHeightRestored;

  WalletState({
    required WalletLifecycleManager lifecycle,
    required SignalHub signalHub,
    HasStoredWallet? hasStoredWallet,
    ClearStoredWallet? clearStoredWallet,
  }) : lifecycle = lifecycle,
       _signalHub = signalHub,
       _hasStoredWallet = hasStoredWallet,
       _clearStoredWallet = clearStoredWallet {
    seedController.addListener(_onSeedChanged);
    passphraseController.addListener(_onPassphraseChanged);
    viewKeyController.addListener(_onViewOnlyKeysChanged);
    spendKeyController.addListener(_onViewOnlyKeysChanged);
    blockHeightController.addListener(_onBlockHeightChanged);

    _signalHub.onKeysDerived = _handleKeysDerived;
    _signalHub.onSubaddressDerived = _handleSubaddressDerived;
    _signalHub.onSeedGenerated = _handleSeedGenerated;
    _signalHub.onSeedBirthday = _handleSeedBirthday;
    _signalHub.onBlockHeightFromTimestamp = _handleBlockHeightFromTimestamp;
    _signalHub.onBip39LegacySeed = _handleBip39LegacySeed;
    _signalHub.onFreezeThaw = _handleFreezeThaw;

    unawaited(refreshAvailableWalletsAsync());
  }

  // Delegating getters
  String get walletId => lifecycle.walletId;
  set walletId(String v) => lifecycle.walletId = v;
  List<String> get availableWalletIds => lifecycle.availableWalletIds;
  Map<String, WalletInstance> get openWallets => lifecycle.openWallets;
  String? get activeWalletId => lifecycle.activeWalletId;
  List<WalletInstance> get activeWallets => lifecycle.activeWallets;
  bool get hasActiveWallets => lifecycle.hasActiveWallets;
  int get lowestSyncedHeight => lifecycle.lowestSyncedHeight;
  WalletInstance? get activeWallet => lifecycle.activeWallet;
  int get continuousScanCurrentHeight => lifecycle.continuousScanCurrentHeight;
  set continuousScanCurrentHeight(int v) =>
      lifecycle.continuousScanCurrentHeight = v;

  List<OwnedOutput> get allOutputs => lifecycle.allOutputs;
  set allOutputs(List<OwnedOutput> v) => lifecycle.allOutputs = v;
  List<WalletTransaction> get allTransactions => lifecycle.allTransactions;
  set allTransactions(List<WalletTransaction> v) =>
      lifecycle.allTransactions = v;
  Set<String> get selectedOutputs => lifecycle.selectedOutputs;
  set selectedOutputs(Set<String> v) => lifecycle.selectedOutputs = v;

  int get activeAccount {
    final w = lifecycle.activeWallet;
    return w?.activeAccount ?? 0;
  }

  List<int> get accounts {
    final w = lifecycle.activeWallet;
    return w?.accounts ?? [0];
  }

  // Signal handlers
  void _handleKeysDerived(KeysDerivedResponse msg) {
    Log.debug(_tag, 'Keys derived: success=${msg.success}');
    if (msg.success) {
      derivedAddress = msg.address;
      secretSpendKey = msg.secretSpendKey;
      secretViewKey = msg.secretViewKey;
      publicSpendKey = msg.publicSpendKey;
      publicViewKey = msg.publicViewKey;
      responseError = null;

      final seed = seedController.text.trim();
      final shouldOpenImportedWallet =
          _pendingImportedWalletId != null &&
          _pendingImportedWalletSeed == seed;
      if (seed.isNotEmpty &&
          derivedAddress != null &&
          (lifecycle.activeWallet == null || shouldOpenImportedWallet)) {
        final id = shouldOpenImportedWallet
            ? _pendingImportedWalletId!
            : walletId.isEmpty
            ? 'temp_wallet'
            : walletId;
        openWallet(
          id,
          seed,
          network,
          derivedAddress!,
          importedSpendSecretKey: shouldOpenImportedWallet
              ? _pendingImportedSpendSecretKey
              : null,
          importedViewSecretKey: shouldOpenImportedWallet
              ? _pendingImportedViewSecretKey
              : null,
        );
        if (shouldOpenImportedWallet) {
          _pendingImportedWalletId = null;
          _pendingImportedWalletSeed = null;
          _pendingImportedSpendSecretKey = null;
          _pendingImportedViewSecretKey = null;
        }
      }

      if (seed.isNotEmpty && !seed.startsWith('viewonly:')) {
        GetSeedBirthdayRequest(
          seed: seed,
          passphrase: passphrase,
          bip39AccountIndex: bip39AccountIndex,
        ).sendSignalToRust();
      }
    } else {
      derivedAddress = null;
      secretSpendKey = null;
      secretViewKey = null;
      publicSpendKey = null;
      publicViewKey = null;
      responseError = msg.error ?? 'Unknown error';
      polyseedRestoreHeight = null;
      _pendingImportedWalletId = null;
      _pendingImportedWalletSeed = null;
      _pendingImportedSpendSecretKey = null;
      _pendingImportedViewSecretKey = null;
    }
    notifyListeners();
  }

  void _handleSubaddressDerived(SubaddressDerivedResponse msg) {
    if (msg.success && msg.address.isNotEmpty) {
      if (_pendingSubaddresses.isNotEmpty) {
        final key = _pendingSubaddresses.first;
        subaddresses[key] = msg.address;
        _pendingSubaddresses.remove(key);
      }
      notifyListeners();
    }
  }

  void _handleSeedGenerated(SeedGeneratedResponse msg) {
    if (msg.success) {
      seedController.text = msg.seed;
      validationError = null;
      responseError = null;
      derivedAddress = null;

      if (msg.restoreHeight != null) {
        final timestamp = msg.restoreHeight!;
        if (timestamp > 0) {
          final genesisTimestamp = _getGenesisTimestamp(network);
          final approxHeight = ((timestamp - genesisTimestamp) / 120).toInt();
          final safeHeight = (approxHeight - 720).clamp(0, approxHeight);

          polyseedRestoreHeight = safeHeight;
          blockHeightController.text = safeHeight.toString();
          blockHeightUserEdited = false;

          final nodeUrl = NetworkUtils.normalizeNodeUrl(nodeUrlController.text);
          if (nodeUrl.isNotEmpty) {
            GetBlockHeightFromTimestampRequest(
              timestamp: timestamp,
              nodeUrl: nodeUrl,
            ).sendSignalToRust();
          }
        } else {
          polyseedRestoreHeight = null;
        }
      } else {
        polyseedRestoreHeight = null;
      }
    } else {
      responseError = msg.error ?? 'Failed to generate seed';
      polyseedRestoreHeight = null;
    }
    notifyListeners();
  }

  void _handleSeedBirthday(SeedBirthdayResponse msg) {
    if (msg.success && msg.birthday != null) {
      final timestamp = msg.birthday!;
      if (timestamp > 0) {
        final genesisTimestamp = _getGenesisTimestamp(network);
        final approxHeight = ((timestamp - genesisTimestamp) / 120).toInt();
        final safeHeight = (approxHeight - 720).clamp(0, approxHeight);
        polyseedRestoreHeight = safeHeight;

        final nodeUrl = NetworkUtils.normalizeNodeUrl(nodeUrlController.text);
        if (nodeUrl.isNotEmpty) {
          GetBlockHeightFromTimestampRequest(
            timestamp: timestamp,
            nodeUrl: nodeUrl,
          ).sendSignalToRust();
        }
      } else {
        polyseedRestoreHeight = null;
      }
    } else {
      polyseedRestoreHeight = null;
    }
    notifyListeners();
  }

  void _handleBlockHeightFromTimestamp(BlockHeightFromTimestampResponse msg) {
    if (msg.success) {
      final blockHeight = msg.blockHeight;
      final safeHeight = (blockHeight - 720).clamp(0, blockHeight);
      polyseedRestoreHeight = safeHeight;
      blockHeightController.text = safeHeight.toString();
      blockHeightUserEdited = false;
      notifyListeners();
    }
  }

  void _handleBip39LegacySeed(Bip39LegacySeedResponse msg) {
    if (msg.success) {
      derivedLegacySeed = msg.legacySeed;
      DeriveKeysRequest(
        seed: msg.legacySeed,
        network: network,
        passphrase: passphrase,
        bip39AccountIndex: bip39AccountIndex,
      ).sendSignalToRust();
    } else {
      responseError = msg.error ?? 'BIP39 conversion failed';
      derivedLegacySeed = null;
    }
    notifyListeners();
  }

  void _handleFreezeThaw(FreezeThawResponse msg) {
    // Response from Rust confirming freeze/thaw; UI already updated optimistically
  }

  // Methods
  void generateSeed() {
    Log.info(_tag, 'Generating seed (type=$seedType)');
    validationError = null;
    responseError = null;
    derivedAddress = null;
    secretSpendKey = null;
    secretViewKey = null;
    publicSpendKey = null;
    publicViewKey = null;
    notifyListeners();

    final seedTypeBackend = seedType.contains('polyseed')
        ? 'polyseed'
        : seedType.contains('bip39')
        ? 'bip39'
        : 'classic';
    GenerateSeedRequest(seedType: seedTypeBackend).sendSignalToRust();
  }

  void deriveAddress() {
    // View-only mode: use raw key hex inputs
    if (seedType == 'view-only') {
      final viewKey = viewKeyController.text.trim();
      final spendKey = spendKeyController.text.trim();
      if (viewKey.isEmpty || spendKey.isEmpty) {
        validationError = null;
        responseError = null;
        derivedAddress = null;
        secretSpendKey = null;
        secretViewKey = null;
        publicSpendKey = null;
        publicViewKey = null;
        notifyListeners();
        return;
      }
      if (viewKey.length != 64 || spendKey.length != 64) {
        validationError = 'Keys must be 64 hex characters';
        notifyListeners();
        return;
      }
      validationError = null;
      responseError = null;
      derivedAddress = null;
      notifyListeners();

      final sentinel = 'viewonly:$viewKey:$spendKey';
      seedController.removeListener(_onSeedChanged);
      seedController.text = sentinel;
      seedController.addListener(_onSeedChanged);

      DeriveKeysRequest(
        seed: sentinel,
        network: network,
        passphrase: '',
        bip39AccountIndex: 0,
      ).sendSignalToRust();

      deriveSubaddresses();
      return;
    }

    if (seedController.text.trim().isEmpty) {
      validationError = null;
      responseError = null;
      derivedAddress = null;
      secretSpendKey = null;
      secretViewKey = null;
      publicSpendKey = null;
      publicViewKey = null;
      notifyListeners();
      return;
    }

    validationError = null;
    responseError = null;
    derivedAddress = null;
    secretSpendKey = null;
    secretViewKey = null;
    publicSpendKey = null;
    publicViewKey = null;
    notifyListeners();

    final result = KeyParser.parse(seedController.text);
    if (!result.isValid) {
      validationError = result.error;
      notifyListeners();
      return;
    }

    final words = result.normalizedInput!.split(' ');
    if (seedType.contains('bip39') && words.length == 12) {
      ConvertBip39ToLegacyRequest(
        bip39Mnemonic: result.normalizedInput!,
        accountIndex: bip39AccountIndex,
        passphrase: passphrase,
      ).sendSignalToRust();
      return;
    }

    DeriveKeysRequest(
      seed: result.normalizedInput!,
      network: network,
      passphrase: passphrase,
      bip39AccountIndex: bip39AccountIndex,
    ).sendSignalToRust();

    deriveSubaddresses();
  }

  void deriveSubaddresses() {
    if (seedController.text.trim().isEmpty) return;

    final result = KeyParser.parse(seedController.text);
    if (!result.isValid) return;

    if (activeAccount == -1) {
      for (var account in accounts) {
        final used = _getUsedSubaddresses(account);
        int index = 0;
        int derived = 0;
        while (derived < 3) {
          if (!used.contains(index)) {
            final key = '$account,$index';
            if (!subaddresses.containsKey(key) &&
                !_pendingSubaddresses.contains(key)) {
              _pendingSubaddresses.add(key);
              DeriveSubaddressRequest(
                seed: result.normalizedInput!,
                network: network,
                account: account,
                addressIndex: index,
                passphrase: passphrase,
                bip39AccountIndex: bip39AccountIndex,
              ).sendSignalToRust();
            }
            derived++;
          }
          index++;
        }
      }
    } else {
      final used = _getUsedSubaddresses(activeAccount);
      int index = 0;
      int derived = 0;
      while (derived < 5) {
        if (!used.contains(index)) {
          final key = '$activeAccount,$index';
          if (!subaddresses.containsKey(key) &&
              !_pendingSubaddresses.contains(key)) {
            _pendingSubaddresses.add(key);
            DeriveSubaddressRequest(
              seed: result.normalizedInput!,
              network: network,
              account: activeAccount,
              addressIndex: index,
              passphrase: passphrase,
              bip39AccountIndex: bip39AccountIndex,
            ).sendSignalToRust();
          }
          derived++;
        }
        index++;
      }
    }
  }

  Set<int> _getUsedSubaddresses(int account) {
    final used = <int>{};
    for (var output in allOutputs) {
      if (output.subaddressIndex != null) {
        final subIdx = output.subaddressIndex!;
        if (subIdx.$1 == account) {
          used.add(subIdx.$2);
        }
      }
    }
    return used;
  }

  void _onSeedChanged() {
    if (isRestoringWallet) return;
    if (seedType == 'view-only') return; // view-only keys drive derivation
    _debounceTimer?.cancel();
    derivedLegacySeed = null;

    if (checkIsContinuousScanning?.call() == true) {
      isChangingSeed = true;
      StopScanRequest().sendSignalToRust();
    } else {
      clearWalletState();
    }

    _debounceTimer = Timer(const Duration(milliseconds: 800), () {
      deriveAddress();
    });
  }

  void _onPassphraseChanged() {
    passphrase = passphraseController.text;
    _debounceTimer?.cancel();
    _debounceTimer = Timer(const Duration(milliseconds: 800), () {
      deriveAddress();
    });
  }

  void _onViewOnlyKeysChanged() {
    if (seedType != 'view-only') return;
    _debounceTimer?.cancel();

    if (checkIsContinuousScanning?.call() == true) {
      isChangingSeed = true;
      StopScanRequest().sendSignalToRust();
    } else {
      clearWalletState();
    }

    _debounceTimer = Timer(const Duration(milliseconds: 800), () {
      deriveAddress();
    });
  }

  void _onBlockHeightChanged() {
    if (blockHeightFocusNode.hasFocus) {
      blockHeightUserEdited = true;
    }
  }

  void clearWalletState() {
    Log.info(_tag, 'Clearing wallet state');
    continuousScanCurrentHeight = 0;
    allOutputs = [];
    allTransactions = [];
    selectedOutputs = {};
    polyseedRestoreHeight = null;
    derivedLegacySeed = null;
    pendingSpentKeyImages.clear();
    onWalletStateCleared?.call();
    notifyListeners();
  }

  int applyImportedKeyImages(List<String> keyImages) {
    if (keyImages.isEmpty || allOutputs.isEmpty) return 0;

    final updatedOutputs = List<OwnedOutput>.from(allOutputs);
    final indexed = <({int index, OwnedOutput output})>[];
    for (var i = 0; i < updatedOutputs.length; i++) {
      indexed.add((index: i, output: updatedOutputs[i]));
    }
    indexed.sort((a, b) {
      final byHeight = a.output.blockHeight.compareTo(b.output.blockHeight);
      if (byHeight != 0) return byHeight;
      return a.output.outputIndex.compareTo(b.output.outputIndex);
    });

    final count = keyImages.length < indexed.length
        ? keyImages.length
        : indexed.length;
    for (var i = 0; i < count; i++) {
      updatedOutputs[indexed[i].index] = updatedOutputs[indexed[i].index]
          .copyWith(keyImage: keyImages[i]);
    }

    allOutputs = updatedOutputs;
    pendingSpentKeyImages.removeAll(keyImages);

    final wallet = lifecycle.activeWallet;
    if (wallet != null) {
      wallet.outputs = updatedOutputs;
      wallet.outputsByAccount = reconstructOutputsByAccount(updatedOutputs);
    }

    notifyListeners();
    return count;
  }

  void resetWalletState() {
    Log.info(_tag, 'Resetting wallet state');
    seedController.text = '';
    viewKeyController.clear();
    spendKeyController.clear();
    seedType = '25 word (classic)';
    derivedAddress = null;
    secretSpendKey = null;
    secretViewKey = null;
    publicSpendKey = null;
    publicViewKey = null;
    allOutputs = [];
    allTransactions = [];
    selectedOutputs = {};
    continuousScanCurrentHeight = 0;
    polyseedRestoreHeight = null;
    derivedLegacySeed = null;
    pendingSpentKeyImages.clear();
    onWalletStateCleared?.call();
    notifyListeners();
  }

  /// Detect a `viewonly:<view_hex>:<spend_hex>` sentinel in the seed and
  /// restore the view-only UI state (seedType, key controllers).
  void restoreViewOnlyStateFromSeed(String seed) {
    if (seed.startsWith('viewonly:')) {
      final parts = seed.substring('viewonly:'.length).split(':');
      if (parts.length == 2 && parts[0].length == 64 && parts[1].length == 64) {
        seedType = 'view-only';
        viewKeyController.removeListener(_onViewOnlyKeysChanged);
        spendKeyController.removeListener(_onViewOnlyKeysChanged);
        viewKeyController.text = parts[0];
        spendKeyController.text = parts[1];
        viewKeyController.addListener(_onViewOnlyKeysChanged);
        spendKeyController.addListener(_onViewOnlyKeysChanged);
      }
    }
  }

  /// Prepare an imported mnemonic or view-only sentinel as the next active wallet.
  ///
  /// The address is derived asynchronously by Rust. [_handleKeysDerived] opens
  /// this wallet when the matching response arrives.
  void beginImportedWallet({
    required String id,
    required String seed,
    required String walletNetwork,
    String? spendSecretKey,
    String? viewSecretKey,
  }) {
    _pendingImportedWalletId = id;
    _pendingImportedWalletSeed = seed;
    _pendingImportedSpendSecretKey = spendSecretKey;
    _pendingImportedViewSecretKey = viewSecretKey;
    walletId = id;
    network = walletNetwork;
    if (seed.startsWith('viewonly:')) {
      restoreViewOnlyStateFromSeed(seed);
    } else if (seedType == 'view-only') {
      seedType = '25 word (classic)';
      viewKeyController.clear();
      spendKeyController.clear();
    }
    seedController.text = seed;
    deriveAddress();
  }

  void ensureAccountsExistForOutputs(List<OwnedOutput> outputs) {
    if (lifecycle.activeWallet == null) return;

    int highestAccountIndex = 0;
    for (var output in outputs) {
      if (output.subaddressIndex != null) {
        final accountIndex = output.subaddressIndex!.$1;
        if (accountIndex > highestAccountIndex) {
          highestAccountIndex = accountIndex;
        }
      }
    }

    var wallet = lifecycle.activeWallet!;
    bool updated = false;

    for (int i = 0; i <= highestAccountIndex; i++) {
      if (!wallet.accounts.contains(i)) {
        wallet = wallet.createAccount(i);
        updated = true;
      }
    }

    if (updated) {
      lifecycle.openWallets[wallet.walletId] = wallet;
    }
  }

  void selectAccount(int accountIndex) {
    final w = lifecycle.activeWallet;
    if (w != null) {
      final updatedWallet = w.switchAccount(accountIndex);
      lifecycle.openWallets[w.walletId] = updatedWallet;
      if (accountIndex >= 0) {
        final keysToRemove = subaddresses.keys
            .where((key) => !key.startsWith('$accountIndex,'))
            .toList();
        for (var key in keysToRemove) {
          subaddresses.remove(key);
        }
      }
      notifyListeners();

      if (accountIndex >= 0) {
        deriveSubaddresses();
      }
    }
  }

  void createAccount() {
    final newAccountIndex = accounts.isEmpty ? 0 : accounts.last + 1;
    var aw = lifecycle.activeWallet;

    if (aw == null &&
        seedController.text.trim().isNotEmpty &&
        derivedAddress != null) {
      openWallet(
        walletId,
        seedController.text.trim(),
        network,
        derivedAddress!,
      );
      aw = lifecycle.activeWallet;
    }

    if (aw != null) {
      final updatedWallet = aw.createAccount(newAccountIndex);
      lifecycle.openWallets[aw.walletId] = updatedWallet;
      notifyListeners();
      deriveSubaddresses();
    }
  }

  void toggleAccountScanning(int accountIndex, bool shouldScan) {
    final w = lifecycle.activeWallet;
    if (w != null) {
      final updatedWallet = w.toggleAccountScanning(accountIndex, shouldScan);
      lifecycle.openWallets[w.walletId] = updatedWallet;
      notifyListeners();
    }
  }

  WalletInstance openWallet(
    String id,
    String seed,
    String net,
    String address, {
    List<int>? accounts,
    Map<int, List<OwnedOutput>>? outputsByAccount,
    int? activeAccount,
    String? importedSpendSecretKey,
    String? importedViewSecretKey,
  }) {
    Log.info(_tag, 'Opening wallet: $id (network=$net)');
    final wallet = lifecycle.openWallet(
      id,
      seed,
      net,
      address,
      importedSpendSecretKey: importedSpendSecretKey,
      importedViewSecretKey: importedViewSecretKey,
    );

    if (accounts != null && accounts.isNotEmpty) {
      final updatedWallet = wallet.copyWith(
        accounts: accounts,
        outputsByAccount: outputsByAccount ?? {},
        activeAccount: activeAccount ?? 0,
      );
      lifecycle.openWallets[id] = updatedWallet;
      lifecycle.activeWalletId = id;
    }

    derivedAddress = address;
    notifyListeners();
    updateBlockHeightFromWallets();
    return lifecycle.activeWallet ?? wallet;
  }

  void switchToWallet(String id) {
    Log.info(_tag, 'Switching to wallet: $id');
    final wallet = lifecycle.switchToWallet(id);
    if (wallet == null) return;

    isRestoringWallet = true;
    restoreViewOnlyStateFromSeed(wallet.seed);
    seedController.text = wallet.seed;
    network = wallet.network;
    derivedAddress = wallet.address;
    isRestoringWallet = false;
    onDaemonHeightRestored?.call(
      wallet.daemonHeight > 0 ? wallet.daemonHeight : null,
    );
    notifyListeners();
  }

  Future<void> switchWallet(
    String newWalletId, {
    required Future<void> Function() loadWalletData,
  }) async {
    final result = await lifecycle.switchWalletAsync(newWalletId);

    switch (result) {
      case SwitchResult.alreadyCurrent:
        return;
      case SwitchResult.switchedToOpen:
        final wallet = lifecycle.activeWallet!;
        isRestoringWallet = true;
        restoreViewOnlyStateFromSeed(wallet.seed);
        seedController.text = wallet.seed;
        network = wallet.network;
        derivedAddress = wallet.address;
        isRestoringWallet = false;
        onDaemonHeightRestored?.call(
          wallet.daemonHeight > 0 ? wallet.daemonHeight : null,
        );
        notifyListeners();
        return;
      case SwitchResult.needsLoad:
        await loadWalletData();
        return;
      case SwitchResult.reset:
        resetWalletState();
        return;
    }
  }

  Future<void> closeWallet(
    BuildContext context,
    String wId, {
    required Future<void> Function() saveWalletData,
    required void Function() pauseScan,
    required void Function() startScan,
    required bool isContinuousScanning,
  }) async {
    if (openWallets[wId] == null) return;

    final shouldSave = await CloseWalletDialog.show(context, wId);
    if (shouldSave == null || !context.mounted) return;

    Log.info(_tag, 'Closing wallet: $wId (save=$shouldSave)');
    if (shouldSave) {
      final previousWalletId = activeWalletId;
      switchToWallet(wId);
      await saveWalletData();
      if (previousWalletId != null && previousWalletId != wId) {
        switchToWallet(previousWalletId);
      }
    }

    final closeResult = lifecycle.closeWallet(wId);
    if (!closeResult.found) return;

    if (closeResult.switchedTo != null) {
      isRestoringWallet = true;
      restoreViewOnlyStateFromSeed(closeResult.switchedTo!.seed);
      seedController.text = closeResult.switchedTo!.seed;
      network = closeResult.switchedTo!.network;
      derivedAddress = closeResult.switchedTo!.address;
      isRestoringWallet = false;
      onDaemonHeightRestored?.call(
        closeResult.switchedTo!.daemonHeight > 0
            ? closeResult.switchedTo!.daemonHeight
            : null,
      );
    } else {
      onWalletStateCleared?.call();
    }
    notifyListeners();

    updateBlockHeightFromWallets();

    if (isContinuousScanning && hasActiveWallets) {
      pauseScan();
      Future.delayed(const Duration(milliseconds: 500), () {
        startScan();
      });
    }
  }

  void startNewWallet() {
    Log.info(_tag, 'Starting new wallet');
    lifecycle.startNewWallet();
    viewKeyController.clear();
    spendKeyController.clear();
    resetWalletState();
    notifyListeners();

    unawaited(_clearTempWalletIfStored());

    showSnackBar?.call(
      'Ready for new wallet - generate or enter a seed phrase',
    );
  }

  void refreshAvailableWallets() {
    unawaited(refreshAvailableWalletsAsync());
  }

  Future<void> refreshAvailableWalletsAsync() async {
    await lifecycle.refreshAvailableWalletsAsync();
    notifyListeners();
  }

  Future<void> _clearTempWalletIfStored() async {
    final hasStoredWallet = _hasStoredWallet;
    final clearStoredWallet = _clearStoredWallet;
    if (hasStoredWallet == null || clearStoredWallet == null) return;
    if (await hasStoredWallet('temp_wallet')) {
      await clearStoredWallet('temp_wallet');
    }
  }

  void updateBlockHeightFromWallets() {
    if (hasActiveWallets) {
      final lowestHeight = lowestSyncedHeight;
      if (lowestHeight > 0 && !blockHeightUserEdited) {
        blockHeightController.text = lowestHeight.toString();
        notifyListeners();
      }
    }
  }

  Map<int, List<OwnedOutput>> reconstructOutputsByAccount(
    List<OwnedOutput> outputs,
  ) {
    final map = <int, List<OwnedOutput>>{};
    for (final output in outputs) {
      final account = output.subaddressIndex?.$1 ?? 0;
      (map[account] ??= []).add(output);
    }
    if (map.isEmpty) {
      map[0] = outputs;
    }
    return map;
  }

  int _getGenesisTimestamp(String net) {
    switch (net.toLowerCase()) {
      case 'mainnet':
        return 1397818193;
      case 'stagenet':
        return 1458748658;
      case 'testnet':
        return 1410295020;
      default:
        return 1397818193;
    }
  }

  void notify() => notifyListeners();

  @override
  void dispose() {
    _debounceTimer?.cancel();
    seedController.removeListener(_onSeedChanged);
    passphraseController.removeListener(_onPassphraseChanged);
    viewKeyController.removeListener(_onViewOnlyKeysChanged);
    spendKeyController.removeListener(_onViewOnlyKeysChanged);
    blockHeightController.removeListener(_onBlockHeightChanged);
    seedController.dispose();
    passphraseController.dispose();
    viewKeyController.dispose();
    spendKeyController.dispose();
    nodeUrlController.dispose();
    blockHeightController.dispose();
    blockHeightFocusNode.dispose();
    super.dispose();
  }
}
