// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class FreezeOutputRequest {
  const FreezeOutputRequest({
    required this.keyImage,
  });

  static FreezeOutputRequest deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = FreezeOutputRequest(
      keyImage: deserializer.deserializeString(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static FreezeOutputRequest bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = FreezeOutputRequest.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String keyImage;

  FreezeOutputRequest copyWith({
    String? keyImage,
  }) {
    return FreezeOutputRequest(
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

    return other is FreezeOutputRequest
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

    return fullString ?? 'FreezeOutputRequest';
  }
}

extension FreezeOutputRequestDartSignalExt on FreezeOutputRequest {
  /// Sends the signal to Rust.
  /// Passing data from Rust to Dart involves a memory copy
  /// because Rust cannot own data managed by Dart's garbage collector.
  void sendSignalToRust() {
    final messageBytes = bincodeSerialize();
    final binary = Uint8List(0);
    sendDartSignal(
      'rinf_send_dart_signal_freeze_output_request',
      messageBytes,
      binary,
    );
  }
}
