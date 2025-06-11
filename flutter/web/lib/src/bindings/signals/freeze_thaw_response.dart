// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class FreezeThawResponse {
  /// An async broadcast stream that listens for signals from Rust.
  /// It supports multiple subscriptions.
  /// Make sure to cancel the subscription when it's no longer needed,
  /// such as when a widget is disposed.
  static final rustSignalStream =
      _freezeThawResponseStreamController.stream.asBroadcastStream();
        
  /// The latest signal value received from Rust.
  /// This is updated every time a new signal is received.
  /// It can be null if no signals have been received yet.
  static RustSignalPack<FreezeThawResponse>? latestRustSignal = null;

  const FreezeThawResponse({
    required this.success,
    required this.keyImage,
    required this.frozen,
  });

  static FreezeThawResponse deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = FreezeThawResponse(
      success: deserializer.deserializeBool(),
      keyImage: deserializer.deserializeString(),
      frozen: deserializer.deserializeBool(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static FreezeThawResponse bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = FreezeThawResponse.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final bool success;
  final String keyImage;
  final bool frozen;

  FreezeThawResponse copyWith({
    bool? success,
    String? keyImage,
    bool? frozen,
  }) {
    return FreezeThawResponse(
      success: success ?? this.success,
      keyImage: keyImage ?? this.keyImage,
      frozen: frozen ?? this.frozen,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeBool(success);
    serializer.serializeString(keyImage);
    serializer.serializeBool(frozen);
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

    return other is FreezeThawResponse
      && success == other.success
      && keyImage == other.keyImage
      && frozen == other.frozen;
  }

  @override
  int get hashCode => Object.hash(
        success,
        keyImage,
        frozen,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'success: $success, '
        'keyImage: $keyImage, '
        'frozen: $frozen'
        ')';
      return true;
    }());

    return fullString ?? 'FreezeThawResponse';
  }
}

final _freezeThawResponseStreamController =
    StreamController<RustSignalPack<FreezeThawResponse>>();
