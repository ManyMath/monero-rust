// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class BlockHashesResponse {
  static final rustSignalStream =
      _blockHashesResponseStreamController.stream.asBroadcastStream();

  static RustSignalPack<BlockHashesResponse>? latestRustSignal = null;

  const BlockHashesResponse({
    required this.success,
    this.error,
    this.blockHashesJson,
  });

  static BlockHashesResponse deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = BlockHashesResponse(
      success: deserializer.deserializeBool(),
      error: TraitHelpers.deserializeOptionStr(deserializer),
      blockHashesJson: TraitHelpers.deserializeOptionStr(deserializer),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static BlockHashesResponse bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = BlockHashesResponse.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final bool success;
  final String? error;
  final String? blockHashesJson;

  BlockHashesResponse copyWith({
    bool? success,
    String? Function()? error,
    String? Function()? blockHashesJson,
  }) {
    return BlockHashesResponse(
      success: success ?? this.success,
      error: error == null ? this.error : error(),
      blockHashesJson: blockHashesJson == null ? this.blockHashesJson : blockHashesJson(),
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeBool(success);
    TraitHelpers.serializeOptionStr(error, serializer);
    TraitHelpers.serializeOptionStr(blockHashesJson, serializer);
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

    return other is BlockHashesResponse
      && success == other.success
      && error == other.error
      && blockHashesJson == other.blockHashesJson;
  }

  @override
  int get hashCode => Object.hash(
        success,
        error,
        blockHashesJson,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'success: $success, '
        'error: $error, '
        'blockHashesJson: $blockHashesJson'
        ')';
      return true;
    }());

    return fullString ?? 'BlockHashesResponse';
  }
}

final _blockHashesResponseStreamController =
    StreamController<RustSignalPack<BlockHashesResponse>>();
