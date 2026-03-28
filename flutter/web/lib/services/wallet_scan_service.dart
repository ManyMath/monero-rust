import '../src/ffi/signal_types.dart';
import '../utils/key_parser.dart';
import '../utils/network_utils.dart';
import '../models/wallet_instance.dart';

/// Service for handling blockchain scanning operations
class WalletScanService {
  /// Scan a single block for a wallet
  static ScanBlockValidation validateScanBlock({
    required String seed,
    required String blockHeight,
    required String nodeUrl,
  }) {
    if (seed.trim().isEmpty) {
      return ScanBlockValidation.error('Please enter a seed phrase first');
    }

    final result = KeyParser.parse(seed);
    if (!result.isValid) {
      return ScanBlockValidation.error('Invalid seed phrase: ${result.error}');
    }

    if (blockHeight.trim().isEmpty) {
      return ScanBlockValidation.error('Please enter a block height');
    }

    final parsedHeight = int.tryParse(blockHeight.trim());
    if (parsedHeight == null || parsedHeight < 0) {
      return ScanBlockValidation.error('Invalid block height');
    }

    if (nodeUrl.trim().isEmpty) {
      return ScanBlockValidation.error('Please enter a node URL');
    }

    return ScanBlockValidation.success(
      normalizedSeed: result.normalizedInput!,
      blockHeight: parsedHeight,
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
    );
  }

  /// Execute a single block scan
  static void scanBlock({
    required String seed,
    required int blockHeight,
    required String nodeUrl,
    required String network,
    List<int>? accountsToScan,
    int accountLookahead = 0,
    int subaddressLookahead = 0,
    String passphrase = '',
    int bip39AccountIndex = 0,
  }) {
    ScanBlockRequest(
      nodeUrl: nodeUrl,
      blockHeight: blockHeight,
      seed: seed,
      network: network,
      accountsToScan: accountsToScan,
      accountLookahead: accountLookahead,
      subaddressLookahead: subaddressLookahead,
      passphrase: passphrase,
      bip39AccountIndex: bip39AccountIndex,
    ).sendSignalToRust();
  }

  /// Validate continuous scan parameters
  static ContinuousScanValidation validateContinuousScan({
    required String seed,
    required String blockHeight,
    required String nodeUrl,
    required List<WalletInstance> activeWallets,
  }) {
    if (activeWallets.isEmpty) {
      if (seed.trim().isEmpty) {
        return ContinuousScanValidation.error(
          'Please enter a seed phrase or load a wallet first',
        );
      }

      final result = KeyParser.parse(seed);
      if (!result.isValid) {
        return ContinuousScanValidation.error(
          'Invalid seed phrase: ${result.error}',
        );
      }
    }

    final heightStr = blockHeight.trim();
    if (heightStr.isEmpty) {
      return ContinuousScanValidation.error('Please enter a block height');
    }

    final parsedHeight = int.tryParse(heightStr);
    if (parsedHeight == null || parsedHeight < 0) {
      return ContinuousScanValidation.error('Invalid block height');
    }

    if (nodeUrl.trim().isEmpty) {
      return ContinuousScanValidation.error('Please enter a node URL');
    }

    return ContinuousScanValidation.success(
      startHeight: parsedHeight,
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
    );
  }

  /// Start continuous blockchain scan
  static void startContinuousScan({
    required String nodeUrl,
    required int startHeight,
    required List<WalletInstance> walletsToScan,
    String? seed,
    String? network,
    int accountLookahead = 0,
    int subaddressLookahead = 0,
    List<int>? accountsToScan,
    String passphrase = '',
    int bip39AccountIndex = 0,
  }) {
    if (walletsToScan.length > 1) {
      final walletConfigs = walletsToScan
          .map(
            (w) => w.toWalletConfig(
              subaddressLookahead: subaddressLookahead,
              passphrase: passphrase,
              bip39AccountIndex: bip39AccountIndex,
            ),
          )
          .toList();

      StartMultiWalletScanRequest(
        nodeUrl: nodeUrl,
        startHeight: startHeight,
        wallets: walletConfigs,
      ).sendSignalToRust();
    } else if (walletsToScan.isNotEmpty) {
      final wallet = walletsToScan.first;
      StartContinuousScanRequest(
        nodeUrl: nodeUrl,
        startHeight: startHeight,
        seed: wallet.seed,
        network: wallet.network,
        accountLookahead: accountLookahead,
        subaddressLookahead: subaddressLookahead,
        accountsToScan: _sortedAccounts(wallet.scanningAccounts),
        passphrase: passphrase,
        bip39AccountIndex: bip39AccountIndex,
      ).sendSignalToRust();
    } else if (seed != null && network != null) {
      StartContinuousScanRequest(
        nodeUrl: nodeUrl,
        startHeight: startHeight,
        seed: seed,
        network: network,
        accountLookahead: accountLookahead,
        subaddressLookahead: subaddressLookahead,
        accountsToScan: accountsToScan,
        passphrase: passphrase,
        bip39AccountIndex: bip39AccountIndex,
      ).sendSignalToRust();
    }
  }

  static List<int> _sortedAccounts(Set<int> accounts) =>
      accounts.toList()..sort();

  /// Pause continuous scan
  static void pauseContinuousScan() {
    StopScanRequest().sendSignalToRust();
  }

  /// Query daemon height
  static void queryDaemonHeight(String nodeUrl) {
    QueryDaemonHeightRequest(
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
    ).sendSignalToRust();
  }

  /// Validate mempool scan parameters
  static MempoolScanValidation validateMempoolScan({
    required String seed,
    required String nodeUrl,
  }) {
    if (seed.trim().isEmpty) {
      return MempoolScanValidation.error('Please enter a seed phrase first');
    }

    final result = KeyParser.parse(seed);
    if (!result.isValid) {
      return MempoolScanValidation.error(
        'Invalid seed phrase: ${result.error}',
      );
    }

    if (nodeUrl.trim().isEmpty) {
      return MempoolScanValidation.error('Please enter a node URL');
    }

    return MempoolScanValidation.success(
      normalizedSeed: result.normalizedInput!,
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
    );
  }

  /// Scan mempool for unconfirmed transactions
  static void scanMempool({
    required String seed,
    required String nodeUrl,
    required String network,
    int accountLookahead = 0,
    int subaddressLookahead = 0,
    List<int>? accountsToScan,
    String passphrase = '',
    int bip39AccountIndex = 0,
  }) {
    MempoolScanRequest(
      nodeUrl: nodeUrl,
      seed: seed,
      network: network,
      accountLookahead: accountLookahead,
      subaddressLookahead: subaddressLookahead,
      accountsToScan: accountsToScan,
      passphrase: passphrase,
      bip39AccountIndex: bip39AccountIndex,
    ).sendSignalToRust();
  }
}

/// Validation result for single block scan
class ScanBlockValidation {
  final bool isValid;
  final String? error;
  final String? normalizedSeed;
  final int? blockHeight;
  final String? nodeUrl;

  ScanBlockValidation._({
    required this.isValid,
    this.error,
    this.normalizedSeed,
    this.blockHeight,
    this.nodeUrl,
  });

  factory ScanBlockValidation.success({
    required String normalizedSeed,
    required int blockHeight,
    required String nodeUrl,
  }) {
    return ScanBlockValidation._(
      isValid: true,
      normalizedSeed: normalizedSeed,
      blockHeight: blockHeight,
      nodeUrl: nodeUrl,
    );
  }

  factory ScanBlockValidation.error(String error) {
    return ScanBlockValidation._(isValid: false, error: error);
  }
}

/// Validation result for continuous scan
class ContinuousScanValidation {
  final bool isValid;
  final String? error;
  final int? startHeight;
  final String? nodeUrl;

  ContinuousScanValidation._({
    required this.isValid,
    this.error,
    this.startHeight,
    this.nodeUrl,
  });

  factory ContinuousScanValidation.success({
    required int startHeight,
    required String nodeUrl,
  }) {
    return ContinuousScanValidation._(
      isValid: true,
      startHeight: startHeight,
      nodeUrl: nodeUrl,
    );
  }

  factory ContinuousScanValidation.error(String error) {
    return ContinuousScanValidation._(isValid: false, error: error);
  }
}

/// Validation result for mempool scan
class MempoolScanValidation {
  final bool isValid;
  final String? error;
  final String? normalizedSeed;
  final String? nodeUrl;

  MempoolScanValidation._({
    required this.isValid,
    this.error,
    this.normalizedSeed,
    this.nodeUrl,
  });

  factory MempoolScanValidation.success({
    required String normalizedSeed,
    required String nodeUrl,
  }) {
    return MempoolScanValidation._(
      isValid: true,
      normalizedSeed: normalizedSeed,
      nodeUrl: nodeUrl,
    );
  }

  factory MempoolScanValidation.error(String error) {
    return MempoolScanValidation._(isValid: false, error: error);
  }
}
