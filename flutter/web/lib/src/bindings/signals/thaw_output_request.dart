// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class ThawOutputRequest {
  const ThawOutputRequest({
    required this.keyImage,
  });

  static ThawOutputRequest deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = ThawOutputRequest(
      keyImage: deserializer.deserializeString(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static ThawOutputRequest bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = ThawOutputRequest.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String keyImage;

  ThawOutputRequest copyWith({
    String? keyImage,
  }) {
    return ThawOutputRequest(
      keyImage: keyImage ?? this.keyImage,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(keyImage);
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

    return other is ThawOutputRequest
      && keyImage == other.keyImage;
  }

  @override
  int get hashCode => keyImage.hashCode;

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'keyImage: $keyImage'
        ')';
      return true;
    }());

    return fullString ?? 'ThawOutputRequest';
  }
}

extension ThawOutputRequestDartSignalExt on ThawOutputRequest {
  /// Sends the signal to Rust.
  /// Passing data from Rust to Dart involves a memory copy
  /// because Rust cannot own data managed by Dart's garbage collector.
  void sendSignalToRust() {
    final messageBytes = bincodeSerialize();
    final binary = Uint8List(0);
    sendDartSignal(
      'rinf_send_dart_signal_thaw_output_request',
      messageBytes,
      binary,
    );
  }
}
