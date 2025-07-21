import 'dart:async';
import 'signal_types.dart';

typedef SignalCallback<T> = void Function(T message);

class SignalHub {
  SignalCallback<KeysDerivedResponse>? onKeysDerived;
  SignalCallback<SubaddressDerivedResponse>? onSubaddressDerived;
  SignalCallback<SeedGeneratedResponse>? onSeedGenerated;
  SignalCallback<SeedBirthdayResponse>? onSeedBirthday;
  SignalCallback<BlockHeightFromTimestampResponse>? onBlockHeightFromTimestamp;
  SignalCallback<BlockScanResponse>? onBlockScan;
  SignalCallback<DaemonHeightResponse>? onDaemonHeight;
  SignalCallback<SyncProgressResponse>? onSyncProgress;
  SignalCallback<SpentStatusUpdatedResponse>? onSpentStatusUpdated;
  SignalCallback<MempoolScanResponse>? onMempoolScan;
  SignalCallback<MultiWalletScanResponse>? onMultiWalletScan;
  SignalCallback<ReorgDetectedResponse>? onReorgDetected;
  SignalCallback<DoubleSpendDetectedResponse>? onDoubleSpendDetected;
  SignalCallback<TransactionCreatedResponse>? onTransactionCreated;
  SignalCallback<TransactionBroadcastResponse>? onTransactionBroadcast;
  SignalCallback<Bip39LegacySeedResponse>? onBip39LegacySeed;
  SignalCallback<FreezeThawResponse>? onFreezeThaw;
  SignalCallback<TransactionStatusUpdate>? onTransactionStatusUpdate;

  final List<StreamSubscription> _subscriptions = [];

  void start() {
    _subscriptions.addAll([
      KeysDerivedResponse.stream.listen((msg) => onKeysDerived?.call(msg)),
      SubaddressDerivedResponse.stream.listen((msg) => onSubaddressDerived?.call(msg)),
      SeedGeneratedResponse.stream.listen((msg) => onSeedGenerated?.call(msg)),
      SeedBirthdayResponse.stream.listen((msg) => onSeedBirthday?.call(msg)),
      BlockHeightFromTimestampResponse.stream.listen((msg) => onBlockHeightFromTimestamp?.call(msg)),
      BlockScanResponse.stream.listen((msg) => onBlockScan?.call(msg)),
      DaemonHeightResponse.stream.listen((msg) => onDaemonHeight?.call(msg)),
      SyncProgressResponse.stream.listen((msg) => onSyncProgress?.call(msg)),
      SpentStatusUpdatedResponse.stream.listen((msg) => onSpentStatusUpdated?.call(msg)),
      MempoolScanResponse.stream.listen((msg) => onMempoolScan?.call(msg)),
      MultiWalletScanResponse.stream.listen((msg) => onMultiWalletScan?.call(msg)),
      ReorgDetectedResponse.stream.listen((msg) => onReorgDetected?.call(msg)),
      DoubleSpendDetectedResponse.stream.listen((msg) => onDoubleSpendDetected?.call(msg)),
      TransactionCreatedResponse.stream.listen((msg) => onTransactionCreated?.call(msg)),
      TransactionBroadcastResponse.stream.listen((msg) => onTransactionBroadcast?.call(msg)),
      Bip39LegacySeedResponse.stream.listen((msg) => onBip39LegacySeed?.call(msg)),
      FreezeThawResponse.stream.listen((msg) => onFreezeThaw?.call(msg)),
      TransactionStatusUpdate.stream.listen((msg) => onTransactionStatusUpdate?.call(msg)),
    ]);
  }

  void dispose() {
    for (final s in _subscriptions) {
      s.cancel();
    }
    _subscriptions.clear();
  }
}
