import 'dart:async';
import 'package:flutter/material.dart';
import 'package:monero_extension/utils/network_utils.dart';
import '../src/ffi/signal_types.dart';
import '../src/logging.dart';
import '../utils/key_parser.dart';
import '../models/wallet_instance.dart';
import '../models/wallet_transaction.dart';
import '../services/wallet_lifecycle_manager.dart';
import '../services/wallet_persistence_browser.dart';
import '../widgets/close_wallet_dialog.dart';
import '../src/ffi/signal_hub.dart';

class WalletState extends ChangeNotifier {
  static const _tag = 'WalletState';
  final WalletLifecycleManager lifecycle;
  final SignalHub _signalHub;

  final seedController = TextEditingController();
  final passphraseController = TextEditingController();
  final nodeUrlController = TextEditingController(text: 'http://127.0.0.1:38081');
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
  })  : lifecycle = lifecycle,
        _signalHub = signalHub {
    seedController.addListener(_onSeedChanged);
    passphraseController.addListener(_onPassphraseChanged);
    blockHeightController.addListener(_onBlockHeightChanged);

    _signalHub.onKeysDerived = _handleKeysDerived;
    _signalHub.onSubaddressDerived = _handleSubaddressDerived;
    _signalHub.onSeedGenerated = _handleSeedGenerated;
    _signalHub.onSeedBirthday = _handleSeedBirthday;
    _signalHub.onBlockHeightFromTimestamp = _handleBlockHeightFromTimestamp;
    _signalHub.onBip39LegacySeed = _handleBip39LegacySeed;
    _signalHub.onFreezeThaw = _handleFreezeThaw;

    refreshAvailableWallets();
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
  set continuousScanCurrentHeight(int v) => lifecycle.continuousScanCurrentHeight = v;

  List<OwnedOutput> get allOutputs => lifecycle.allOutputs;
  set allOutputs(List<OwnedOutput> v) => lifecycle.allOutputs = v;
  List<WalletTransaction> get allTransactions => lifecycle.allTransactions;
  set allTransactions(List<WalletTransaction> v) => lifecycle.allTransactions = v;
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
      if (seed.isNotEmpty && derivedAddress != null && lifecycle.activeWallet == null) {
        openWallet(walletId.isEmpty ? 'temp_wallet' : walletId, seed, network, derivedAddress!);
      }

      if (seed.isNotEmpty) {
        GetSeedBirthdayRequest(seed: seed, passphrase: passphrase, bip39AccountIndex: bip39AccountIndex).sendSignalToRust();
      }
    } else {
      derivedAddress = null;
      secretSpendKey = null;
      secretViewKey = null;
      publicSpendKey = null;
      publicViewKey = null;
      responseError = msg.error ?? 'Unknown error';
      polyseedRestoreHeight = null;
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
      DeriveKeysRequest(seed: msg.legacySeed, network: network, passphrase: passphrase, bip39AccountIndex: bip39AccountIndex).sendSignalToRust();
    } else {
      responseError = msg.error ?? 'BIP39 conversion failed';
      derivedLegacySeed = null;
    }
    notifyListeners();
  }

  void _handleFreezeThaw(FreezeThawResponse msg) {
    // Response from Rust confirming freeze/thaw — UI already updated optimistically
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
            if (!subaddresses.containsKey(key) && !_pendingSubaddresses.contains(key)) {
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
          if (!subaddresses.containsKey(key) && !_pendingSubaddresses.contains(key)) {
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

  void resetWalletState() {
    Log.info(_tag, 'Resetting wallet state');
    seedController.text = '';
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
        final keysToRemove = subaddresses.keys.where((key) =>
            !key.startsWith('$accountIndex,')).toList();
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

    if (aw == null && seedController.text.trim().isNotEmpty && derivedAddress != null) {
      openWallet(walletId, seedController.text.trim(), network, derivedAddress!);
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

  WalletInstance openWallet(String id, String seed, String net, String address, {
    List<int>? accounts,
    Map<int, List<OwnedOutput>>? outputsByAccount,
    int? activeAccount,
  }) {
    Log.info(_tag, 'Opening wallet: $id (network=$net)');
    final wallet = lifecycle.openWallet(id, seed, net, address);

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
    seedController.text = wallet.seed;
    network = wallet.network;
    derivedAddress = wallet.address;
    isRestoringWallet = false;
    onDaemonHeightRestored?.call(wallet.daemonHeight > 0 ? wallet.daemonHeight : null);
    notifyListeners();
  }

  Future<void> switchWallet(String newWalletId, {
    required Future<void> Function() loadWalletData,
  }) async {
    final result = lifecycle.switchWallet(newWalletId);

    switch (result) {
      case SwitchResult.alreadyCurrent:
        return;
      case SwitchResult.switchedToOpen:
        final wallet = lifecycle.activeWallet!;
        isRestoringWallet = true;
        seedController.text = wallet.seed;
        network = wallet.network;
        derivedAddress = wallet.address;
        isRestoringWallet = false;
        onDaemonHeightRestored?.call(wallet.daemonHeight > 0 ? wallet.daemonHeight : null);
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

  Future<void> closeWallet(BuildContext context, String wId, {
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
      seedController.text = closeResult.switchedTo!.seed;
      network = closeResult.switchedTo!.network;
      derivedAddress = closeResult.switchedTo!.address;
      isRestoringWallet = false;
      onDaemonHeightRestored?.call(
        closeResult.switchedTo!.daemonHeight > 0 ? closeResult.switchedTo!.daemonHeight : null,
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
    resetWalletState();
    notifyListeners();

    if (WalletPersistenceBrowser.hasWalletData('temp_wallet')) {
      WalletPersistenceBrowser.clearWalletData('temp_wallet');
    }

    showSnackBar?.call('Ready for new wallet - generate or enter a seed phrase');
  }

  void refreshAvailableWallets() {
    lifecycle.refreshAvailableWallets();
    notifyListeners();
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

  Map<int, List<OwnedOutput>> reconstructOutputsByAccount(List<OwnedOutput> outputs) {
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
    blockHeightController.removeListener(_onBlockHeightChanged);
    seedController.dispose();
    passphraseController.dispose();
    nodeUrlController.dispose();
    blockHeightController.dispose();
    blockHeightFocusNode.dispose();
    super.dispose();
  }
}
