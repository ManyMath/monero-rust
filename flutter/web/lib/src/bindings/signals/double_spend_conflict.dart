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

  static DoubleSpendConflict bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = DoubleSpendConflict.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String keyImage;
  final Uint64 previousSpentHeight;
  final Uint64 newHeight;

  DoubleSpendConflict copyWith({
    String? keyImage,
    Uint64? previousSpentHeight,
    Uint64? newHeight,
  }) {
    return DoubleSpendConflict(
      keyImage: keyImage ?? this.keyImage,
      previousSpentHeight: previousSpentHeight ?? this.previousSpentHeight,
      newHeight: newHeight ?? this.newHeight,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(keyImage);
    serializer.serializeUint64(previousSpentHeight);
    serializer.serializeUint64(newHeight);
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

    return other is DoubleSpendConflict
      && keyImage == other.keyImage
      && previousSpentHeight == other.previousSpentHeight
      && newHeight == other.newHeight;
  }

  @override
  int get hashCode => Object.hash(
        keyImage,
        previousSpentHeight,
        newHeight,
      );

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
