// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class Bip39LegacySeedResponse {
  static final rustSignalStream =
      _bip39LegacySeedResponseStreamController.stream.asBroadcastStream();

  static RustSignalPack<Bip39LegacySeedResponse>? latestRustSignal = null;

  const Bip39LegacySeedResponse({
    required this.legacySeed,
    required this.success,
    this.error,
  });

  static Bip39LegacySeedResponse deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = Bip39LegacySeedResponse(
      legacySeed: deserializer.deserializeString(),
      success: deserializer.deserializeBool(),
      error: TraitHelpers.deserializeOptionStr(deserializer),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static Bip39LegacySeedResponse bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = Bip39LegacySeedResponse.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String legacySeed;
  final bool success;
  final String? error;

  Bip39LegacySeedResponse copyWith({
    String? legacySeed,
    bool? success,
    String? Function()? error,
  }) {
    return Bip39LegacySeedResponse(
      legacySeed: legacySeed ?? this.legacySeed,
      success: success ?? this.success,
      error: error == null ? this.error : error(),
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(legacySeed);
    serializer.serializeBool(success);
    TraitHelpers.serializeOptionStr(error, serializer);
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

    return other is Bip39LegacySeedResponse
      && legacySeed == other.legacySeed
      && success == other.success
      && error == other.error;
  }

  @override
  int get hashCode => Object.hash(
        legacySeed,
        success,
        error,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'legacySeed: $legacySeed, '
        'success: $success, '
        'error: $error'
        ')';
      return true;
    }());

    return fullString ?? 'Bip39LegacySeedResponse';
  }
}

final _bip39LegacySeedResponseStreamController =
    StreamController<RustSignalPack<Bip39LegacySeedResponse>>();
