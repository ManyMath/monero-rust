// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class DoubleSpendDetectedResponse {
  /// An async broadcast stream that listens for signals from Rust.
  /// It supports multiple subscriptions.
  /// Make sure to cancel the subscription when it's no longer needed,
  /// such as when a widget is disposed.
  static final rustSignalStream =
      _doubleSpendDetectedResponseStreamController.stream.asBroadcastStream();
        
  /// The latest signal value received from Rust.
  /// This is updated every time a new signal is received.
  /// It can be null if no signals have been received yet.
  static RustSignalPack<DoubleSpendDetectedResponse>? latestRustSignal = null;

  const DoubleSpendDetectedResponse({
    required this.conflicts,
  });

  static DoubleSpendDetectedResponse deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = DoubleSpendDetectedResponse(
      conflicts: TraitHelpers.deserializeVectorDoubleSpendConflict(deserializer),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static DoubleSpendDetectedResponse bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = DoubleSpendDetectedResponse.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final List<DoubleSpendConflict> conflicts;

  DoubleSpendDetectedResponse copyWith({
    List<DoubleSpendConflict>? conflicts,
  }) {
    return DoubleSpendDetectedResponse(
      conflicts: conflicts ?? this.conflicts,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    TraitHelpers.serializeVectorDoubleSpendConflict(conflicts, serializer);
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

    return other is DoubleSpendDetectedResponse
      && listEquals(conflicts, other.conflicts);
  }

  @override
  int get hashCode => conflicts.hashCode;

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'conflicts: $conflicts'
        ')';
      return true;
    }());

    return fullString ?? 'DoubleSpendDetectedResponse';
  }
}

final _doubleSpendDetectedResponseStreamController =
    StreamController<RustSignalPack<DoubleSpendDetectedResponse>>();
