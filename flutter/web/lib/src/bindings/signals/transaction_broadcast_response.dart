// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class TransactionBroadcastResponse {
  /// An async broadcast stream that listens for signals from Rust.
  /// It supports multiple subscriptions.
  /// Make sure to cancel the subscription when it's no longer needed,
  /// such as when a widget is disposed.
  static final rustSignalStream =
      _transactionBroadcastResponseStreamController.stream.asBroadcastStream();
        
  /// The latest signal value received from Rust.
  /// This is updated every time a new signal is received.
  /// It can be null if no signals have been received yet.
  static RustSignalPack<TransactionBroadcastResponse>? latestRustSignal = null;

  const TransactionBroadcastResponse({
    required this.success,
    this.error,
    this.txId,
    required this.isRetryable,
    required this.isDoubleSpend,
  });

  static TransactionBroadcastResponse deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = TransactionBroadcastResponse(
      success: deserializer.deserializeBool(),
      error: TraitHelpers.deserializeOptionStr(deserializer),
      txId: TraitHelpers.deserializeOptionStr(deserializer),
      isRetryable: deserializer.deserializeBool(),
      isDoubleSpend: deserializer.deserializeBool(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static TransactionBroadcastResponse bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = TransactionBroadcastResponse.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final bool success;
  final String? error;
  final String? txId;
  final bool isRetryable;
  final bool isDoubleSpend;

  TransactionBroadcastResponse copyWith({
    bool? success,
    String? Function()? error,
    String? Function()? txId,
    bool? isRetryable,
    bool? isDoubleSpend,
  }) {
    return TransactionBroadcastResponse(
      success: success ?? this.success,
      error: error == null ? this.error : error(),
      txId: txId == null ? this.txId : txId(),
      isRetryable: isRetryable ?? this.isRetryable,
      isDoubleSpend: isDoubleSpend ?? this.isDoubleSpend,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeBool(success);
    TraitHelpers.serializeOptionStr(error, serializer);
    TraitHelpers.serializeOptionStr(txId, serializer);
    serializer.serializeBool(isRetryable);
    serializer.serializeBool(isDoubleSpend);
    serializer.decreaseContainerDepth();
  }

  Uint8List bincodeSerialize() {
      final serializer = BincodeSerializer();
      serialize(serializer);
      return serializer.bytes;
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    if (other.runtimeType != runtimeType) return false;

    return other is TransactionBroadcastResponse
      && success == other.success
      && error == other.error
      && txId == other.txId
      && isRetryable == other.isRetryable
      && isDoubleSpend == other.isDoubleSpend;
  }

  @override
  int get hashCode => Object.hash(
        success,
        error,
        txId,
        isRetryable,
        isDoubleSpend,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'success: $success, '
        'error: $error, '
        'txId: $txId, '
        'isRetryable: $isRetryable, '
        'isDoubleSpend: $isDoubleSpend'
        ')';
      return true;
    }());

    return fullString ?? 'TransactionBroadcastResponse';
  }
}

final _transactionBroadcastResponseStreamController =
    StreamController<RustSignalPack<TransactionBroadcastResponse>>();
