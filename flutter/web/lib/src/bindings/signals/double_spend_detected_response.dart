// ignore_for_file: type=lint, type=warning
part of 'signals.dart';

@immutable
class DoubleSpendConflict {
  const DoubleSpendConflict({
    required this.keyImage,
    required this.previousSpentHeight,
    required this.newHeight,
  });

  static DoubleSpendConflict deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = DoubleSpendConflict(
      keyImage: deserializer.deserializeString(),
      previousSpentHeight: deserializer.deserializeUint64(),
      newHeight: deserializer.deserializeUint64(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  final String keyImage;
  final Uint64 previousSpentHeight;
  final Uint64 newHeight;

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(keyImage);
    serializer.serializeUint64(previousSpentHeight);
    serializer.serializeUint64(newHeight);
    serializer.decreaseContainerDepth();
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    if (other.runtimeType != runtimeType) return false;

    return other is DoubleSpendConflict
      && keyImage == other.keyImage
      && previousSpentHeight == other.previousSpentHeight
      && newHeight == other.newHeight;
  }

  @override
  int get hashCode => Object.hash(keyImage, previousSpentHeight, newHeight);

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'keyImage: $keyImage, '
        'previousSpentHeight: $previousSpentHeight, '
        'newHeight: $newHeight'
        ')';
      return true;
    }());

    return fullString ?? 'DoubleSpendConflict';
  }
}

@immutable
class DoubleSpendDetectedResponse {
  static final rustSignalStream =
      _doubleSpendDetectedResponseStreamController.stream.asBroadcastStream();

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
  int get hashCode => Object.hashAll(conflicts);

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
