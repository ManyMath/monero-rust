import 'dart:async';
import 'signal_types.dart';
import '../logging.dart';

typedef SignalCallback<T> = void Function(T message);

class SignalHub {
  static const _tag = 'SignalHub';

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
  SignalCallback<UnsignedTransactionCreatedResponse>?
  onUnsignedTransactionCreated;

  final List<StreamSubscription> _subscriptions = [];

  void start() {
    Log.info(_tag, 'Starting signal subscriptions');
    _subscriptions.addAll([
      KeysDerivedResponse.stream.listen((msg) {
        Log.debug(
          _tag,
          'Received KeysDerivedResponse (success=${msg.success})',
        );
        onKeysDerived?.call(msg);
      }),
      SubaddressDerivedResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received SubaddressDerivedResponse');
        onSubaddressDerived?.call(msg);
      }),
      SeedGeneratedResponse.stream.listen((msg) {
        Log.debug(
          _tag,
          'Received SeedGeneratedResponse (success=${msg.success})',
        );
        onSeedGenerated?.call(msg);
      }),
      SeedBirthdayResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received SeedBirthdayResponse');
        onSeedBirthday?.call(msg);
      }),
      BlockHeightFromTimestampResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received BlockHeightFromTimestampResponse');
        onBlockHeightFromTimestamp?.call(msg);
      }),
      BlockScanResponse.stream.listen((msg) {
        Log.debug(
          _tag,
          'Received BlockScanResponse (height=${msg.blockHeight})',
        );
        onBlockScan?.call(msg);
      }),
      DaemonHeightResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received DaemonHeightResponse');
        onDaemonHeight?.call(msg);
      }),
      SyncProgressResponse.stream.listen((msg) {
        Log.debug(
          _tag,
          'Received SyncProgressResponse (height=${msg.currentHeight}/${msg.daemonHeight})',
        );
        onSyncProgress?.call(msg);
      }),
      SpentStatusUpdatedResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received SpentStatusUpdatedResponse');
        onSpentStatusUpdated?.call(msg);
      }),
      MempoolScanResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received MempoolScanResponse');
        onMempoolScan?.call(msg);
      }),
      MultiWalletScanResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received MultiWalletScanResponse');
        onMultiWalletScan?.call(msg);
      }),
      ReorgDetectedResponse.stream.listen((msg) {
        Log.warn(
          _tag,
          'Received ReorgDetectedResponse (split=${msg.splitHeight})',
        );
        onReorgDetected?.call(msg);
      }),
      DoubleSpendDetectedResponse.stream.listen((msg) {
        Log.warn(_tag, 'Received DoubleSpendDetectedResponse');
        onDoubleSpendDetected?.call(msg);
      }),
      TransactionCreatedResponse.stream.listen((msg) {
        Log.info(
          _tag,
          'Received TransactionCreatedResponse (success=${msg.success})',
        );
        onTransactionCreated?.call(msg);
      }),
      TransactionBroadcastResponse.stream.listen((msg) {
        Log.info(
          _tag,
          'Received TransactionBroadcastResponse (success=${msg.success})',
        );
        onTransactionBroadcast?.call(msg);
      }),
      Bip39LegacySeedResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received Bip39LegacySeedResponse');
        onBip39LegacySeed?.call(msg);
      }),
      FreezeThawResponse.stream.listen((msg) {
        Log.debug(_tag, 'Received FreezeThawResponse');
        onFreezeThaw?.call(msg);
      }),
      TransactionStatusUpdate.stream.listen((msg) {
        Log.debug(_tag, 'Received TransactionStatusUpdate');
        onTransactionStatusUpdate?.call(msg);
      }),
      UnsignedTransactionCreatedResponse.stream.listen((msg) {
        Log.info(
          _tag,
          'Received UnsignedTransactionCreatedResponse (success=${msg.success})',
        );
        onUnsignedTransactionCreated?.call(msg);
      }),
    ]);
  }

  void dispose() {
    Log.info(_tag, 'Disposing signal subscriptions');
    for (final s in _subscriptions) {
      s.cancel();
    }
    _subscriptions.clear();
  }
}
