import 'dart:async';
import '../src/bindings/bindings.dart';

typedef SignalCallback<T> = void Function(T message);

class RinfSignalHub {
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
      KeysDerivedResponse.rustSignalStream.listen((s) => onKeysDerived?.call(s.message)),
      SubaddressDerivedResponse.rustSignalStream.listen((s) => onSubaddressDerived?.call(s.message)),
      SeedGeneratedResponse.rustSignalStream.listen((s) => onSeedGenerated?.call(s.message)),
      SeedBirthdayResponse.rustSignalStream.listen((s) => onSeedBirthday?.call(s.message)),
      BlockHeightFromTimestampResponse.rustSignalStream.listen((s) => onBlockHeightFromTimestamp?.call(s.message)),
      BlockScanResponse.rustSignalStream.listen((s) => onBlockScan?.call(s.message)),
      DaemonHeightResponse.rustSignalStream.listen((s) => onDaemonHeight?.call(s.message)),
      SyncProgressResponse.rustSignalStream.listen((s) => onSyncProgress?.call(s.message)),
      SpentStatusUpdatedResponse.rustSignalStream.listen((s) => onSpentStatusUpdated?.call(s.message)),
      MempoolScanResponse.rustSignalStream.listen((s) => onMempoolScan?.call(s.message)),
      MultiWalletScanResponse.rustSignalStream.listen((s) => onMultiWalletScan?.call(s.message)),
      ReorgDetectedResponse.rustSignalStream.listen((s) => onReorgDetected?.call(s.message)),
      DoubleSpendDetectedResponse.rustSignalStream.listen((s) => onDoubleSpendDetected?.call(s.message)),
      TransactionCreatedResponse.rustSignalStream.listen((s) => onTransactionCreated?.call(s.message)),
      TransactionBroadcastResponse.rustSignalStream.listen((s) => onTransactionBroadcast?.call(s.message)),
      Bip39LegacySeedResponse.rustSignalStream.listen((s) => onBip39LegacySeed?.call(s.message)),
      FreezeThawResponse.rustSignalStream.listen((s) => onFreezeThaw?.call(s.message)),
      TransactionStatusUpdate.rustSignalStream.listen((s) => onTransactionStatusUpdate?.call(s.message)),
    ]);
  }

  void dispose() {
    for (final s in _subscriptions) {
      s.cancel();
    }
    _subscriptions.clear();
  }
}
