import 'dart:async';
import 'package:flutter/foundation.dart';

class WalletPollingService {
  static final WalletPollingService _instance = WalletPollingService._internal();
  factory WalletPollingService() => _instance;
  WalletPollingService._internal();

  // Timer instances
  Timer? _blockRefreshTimer;
  Timer? _mempoolPollTimer;
  Timer? _mempoolDelayTimer;
  Timer? _countdownTimer;

  // Countdown state as ValueNotifiers (UI can listen without parent setState)
  final ValueNotifier<int> blockRefreshCountdown = ValueNotifier<int>(0);
  final ValueNotifier<int> mempoolCountdown = ValueNotifier<int>(0);

  // Polling intervals
  static const _blockRefreshInterval = Duration(seconds: 90);
  static const _mempoolPollInterval = Duration(seconds: 90);
  static const _mempoolPollOffset = Duration(seconds: 45);

  // Callbacks
  VoidCallback? _onBlockRefresh;
  VoidCallback? _onMempoolPoll;

  bool get isPolling => _blockRefreshTimer != null || _mempoolPollTimer != null;

  void startPolling({
    required VoidCallback onBlockRefresh,
    required VoidCallback onMempoolPoll,
  }) {
    // Cancel any existing timers first
    stopPolling();

    _onBlockRefresh = onBlockRefresh;
    _onMempoolPoll = onMempoolPoll;

    // Initialize countdowns
    blockRefreshCountdown.value = _blockRefreshInterval.inSeconds;
    mempoolCountdown.value = _mempoolPollOffset.inSeconds;

    // Start countdown timer (fires every second)
    _countdownTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (blockRefreshCountdown.value > 0) blockRefreshCountdown.value--;
      if (mempoolCountdown.value > 0) mempoolCountdown.value--;
    });

    // Start block refresh timer
    _blockRefreshTimer = Timer.periodic(_blockRefreshInterval, (_) {
      _onBlockRefresh?.call();
      blockRefreshCountdown.value = _blockRefreshInterval.inSeconds;
    });

    // Start mempool polling with offset to stagger requests
    _mempoolDelayTimer = Timer(_mempoolPollOffset, () {
      _mempoolDelayTimer = null;
      _onMempoolPoll?.call(); // First poll at offset time (45s)
      mempoolCountdown.value = _mempoolPollInterval.inSeconds;

      // Start periodic mempool polling
      _mempoolPollTimer = Timer.periodic(_mempoolPollInterval, (_) {
        _onMempoolPoll?.call();
        mempoolCountdown.value = _mempoolPollInterval.inSeconds;
      });
    });
  }

  void stopPolling() {
    _countdownTimer?.cancel();
    _countdownTimer = null;

    _blockRefreshTimer?.cancel();
    _blockRefreshTimer = null;

    _mempoolDelayTimer?.cancel();
    _mempoolDelayTimer = null;

    _mempoolPollTimer?.cancel();
    _mempoolPollTimer = null;

    blockRefreshCountdown.value = 0;
    mempoolCountdown.value = 0;

    // Clear callbacks
    _onBlockRefresh = null;
    _onMempoolPoll = null;
  }

  void dispose() {
    stopPolling();
    blockRefreshCountdown.dispose();
    mempoolCountdown.dispose();
  }
}
