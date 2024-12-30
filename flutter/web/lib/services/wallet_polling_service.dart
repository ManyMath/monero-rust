import 'dart:async';
import 'package:flutter/foundation.dart';

/// Service for managing background polling timers for wallet operations.
///
/// This service handles:
/// - Block/daemon height refresh polling
/// - Mempool scanning at regular intervals
///
/// The service uses a singleton pattern to ensure only one instance manages
/// all polling timers across the application.
class WalletPollingService {
  static final WalletPollingService _instance = WalletPollingService._internal();
  factory WalletPollingService() => _instance;
  WalletPollingService._internal();

  // Timer instances
  Timer? _blockRefreshTimer;
  Timer? _mempoolPollTimer;
  Timer? _mempoolDelayTimer;
  Timer? _countdownTimer;

  // Countdown state
  int _blockRefreshCountdown = 0;
  int _mempoolCountdown = 0;

  // Polling intervals
  static const _blockRefreshInterval = Duration(seconds: 90);
  static const _mempoolPollInterval = Duration(seconds: 90);
  static const _mempoolPollOffset = Duration(seconds: 45);

  // Callbacks
  VoidCallback? _onBlockRefresh;
  VoidCallback? _onMempoolPoll;
  VoidCallback? _onCountdownUpdate;

  /// Get the current block refresh countdown value
  int get blockRefreshCountdown => _blockRefreshCountdown;

  /// Get the current mempool countdown value
  int get mempoolCountdown => _mempoolCountdown;

  /// Check if polling timers are currently active
  bool get isPolling => _blockRefreshTimer != null || _mempoolPollTimer != null;

  /// Start polling timers with the provided callbacks.
  ///
  /// [onBlockRefresh] - Called when block refresh timer fires
  /// [onMempoolPoll] - Called when mempool poll timer fires
  /// [onCountdownUpdate] - Called every second when countdowns update
  void startPolling({
    required VoidCallback onBlockRefresh,
    required VoidCallback onMempoolPoll,
    VoidCallback? onCountdownUpdate,
  }) {
    debugPrint('[WalletPollingService] Starting polling timers (block: ${_blockRefreshInterval.inSeconds}s, mempool: ${_mempoolPollInterval.inSeconds}s with ${_mempoolPollOffset.inSeconds}s offset)');

    stopPolling();

    _onBlockRefresh = onBlockRefresh;
    _onMempoolPoll = onMempoolPoll;
    _onCountdownUpdate = onCountdownUpdate;

    // Initialize countdowns
    _blockRefreshCountdown = _blockRefreshInterval.inSeconds;
    _mempoolCountdown = _mempoolPollOffset.inSeconds;

    // Start countdown timer (fires every second)
    _countdownTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_blockRefreshCountdown > 0) _blockRefreshCountdown--;
      if (_mempoolCountdown > 0) _mempoolCountdown--;
      _onCountdownUpdate?.call();
    });

    // Start block refresh timer
    _blockRefreshTimer = Timer.periodic(_blockRefreshInterval, (_) {
      _onBlockRefresh?.call();
      _blockRefreshCountdown = _blockRefreshInterval.inSeconds;
      _onCountdownUpdate?.call();
    });

    // Start mempool polling with offset to stagger requests
    _mempoolDelayTimer = Timer(_mempoolPollOffset, () {
      _mempoolDelayTimer = null;
      _onMempoolPoll?.call(); // First poll at offset time (45s)
      _mempoolCountdown = _mempoolPollInterval.inSeconds;
      _onCountdownUpdate?.call();

      // Start periodic mempool polling
      _mempoolPollTimer = Timer.periodic(_mempoolPollInterval, (_) {
        _onMempoolPoll?.call();
        _mempoolCountdown = _mempoolPollInterval.inSeconds;
        _onCountdownUpdate?.call();
      });
    });
  }

  /// Stop all polling timers and cleanup resources.
  void stopPolling() {
    if (_blockRefreshTimer != null || _mempoolPollTimer != null || _mempoolDelayTimer != null) {
      debugPrint('[WalletPollingService] Stopping polling timers');
    }

    _countdownTimer?.cancel();
    _countdownTimer = null;

    _blockRefreshTimer?.cancel();
    _blockRefreshTimer = null;

    _mempoolDelayTimer?.cancel();
    _mempoolDelayTimer = null;

    _mempoolPollTimer?.cancel();
    _mempoolPollTimer = null;

    _blockRefreshCountdown = 0;
    _mempoolCountdown = 0;

    // Clear callbacks
    _onBlockRefresh = null;
    _onMempoolPoll = null;
    _onCountdownUpdate = null;
  }

  /// Dispose of the service and cleanup all resources.
  /// Call this when the service is no longer needed.
  void dispose() {
    stopPolling();
  }
}
