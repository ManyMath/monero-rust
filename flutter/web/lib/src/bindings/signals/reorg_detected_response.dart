// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class ReorgDetectedResponse {
  /// An async broadcast stream that listens for signals from Rust.
  /// It supports multiple subscriptions.
  /// Make sure to cancel the subscription when it's no longer needed,
  /// such as when a widget is disposed.
  static final rustSignalStream =
      _reorgDetectedResponseStreamController.stream.asBroadcastStream();
        
  /// The latest signal value received from Rust.
  /// This is updated every time a new signal is received.
  /// It can be null if no signals have been received yet.
  static RustSignalPack<ReorgDetectedResponse>? latestRustSignal = null;

  const ReorgDetectedResponse({
    required this.splitHeight,
    required this.blocksDetached,
    required this.outputsRemoved,
    required this.outputsUnspent,
    required this.removedKeyImages,
    required this.unspentKeyImages,
  });

  static ReorgDetectedResponse deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = ReorgDetectedResponse(
      splitHeight: deserializer.deserializeUint64(),
      blocksDetached: deserializer.deserializeUint64(),
      outputsRemoved: deserializer.deserializeUint64(),
      outputsUnspent: deserializer.deserializeUint64(),
      removedKeyImages: TraitHelpers.deserializeVectorStr(deserializer),
      unspentKeyImages: TraitHelpers.deserializeVectorStr(deserializer),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static ReorgDetectedResponse bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = ReorgDetectedResponse.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final Uint64 splitHeight;
  final Uint64 blocksDetached;
  final Uint64 outputsRemoved;
  final Uint64 outputsUnspent;
  final List<String> removedKeyImages;
  final List<String> unspentKeyImages;

  ReorgDetectedResponse copyWith({
    Uint64? splitHeight,
    Uint64? blocksDetached,
    Uint64? outputsRemoved,
    Uint64? outputsUnspent,
    List<String>? removedKeyImages,
    List<String>? unspentKeyImages,
  }) {
    return ReorgDetectedResponse(
      splitHeight: splitHeight ?? this.splitHeight,
      blocksDetached: blocksDetached ?? this.blocksDetached,
      outputsRemoved: outputsRemoved ?? this.outputsRemoved,
      outputsUnspent: outputsUnspent ?? this.outputsUnspent,
      removedKeyImages: removedKeyImages ?? this.removedKeyImages,
      unspentKeyImages: unspentKeyImages ?? this.unspentKeyImages,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeUint64(splitHeight);
    serializer.serializeUint64(blocksDetached);
    serializer.serializeUint64(outputsRemoved);
    serializer.serializeUint64(outputsUnspent);
    TraitHelpers.serializeVectorStr(removedKeyImages, serializer);
    TraitHelpers.serializeVectorStr(unspentKeyImages, serializer);
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

    return other is ReorgDetectedResponse
      && splitHeight == other.splitHeight
      && blocksDetached == other.blocksDetached
      && outputsRemoved == other.outputsRemoved
      && outputsUnspent == other.outputsUnspent
      && listEquals(removedKeyImages, other.removedKeyImages)
      && listEquals(unspentKeyImages, other.unspentKeyImages);
  }

  @override
  int get hashCode => Object.hash(
        splitHeight,
        blocksDetached,
        outputsRemoved,
        outputsUnspent,
        removedKeyImages,
        unspentKeyImages,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'splitHeight: $splitHeight, '
        'blocksDetached: $blocksDetached, '
        'outputsRemoved: $outputsRemoved, '
        'outputsUnspent: $outputsUnspent, '
        'removedKeyImages: $removedKeyImages, '
        'unspentKeyImages: $unspentKeyImages'
        ')';
      return true;
    }());

    return fullString ?? 'ReorgDetectedResponse';
  }
}

final _reorgDetectedResponseStreamController =
    StreamController<RustSignalPack<ReorgDetectedResponse>>();
