// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class RestoreWalletDataRequest {
  const RestoreWalletDataRequest({
    required this.seed,
    required this.network,
    required this.outputs,
    required this.daemonHeight,
    required this.currentHeight,
    this.blockHashesJson,
    this.pendingStateJson,
    required this.passphrase,
    required this.bip39AccountIndex,
  });

  static RestoreWalletDataRequest deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = RestoreWalletDataRequest(
      seed: deserializer.deserializeString(),
      network: deserializer.deserializeString(),
      outputs: TraitHelpers.deserializeVectorOwnedOutput(deserializer),
      daemonHeight: deserializer.deserializeUint64(),
      currentHeight: deserializer.deserializeUint64(),
      blockHashesJson: TraitHelpers.deserializeOptionStr(deserializer),
      pendingStateJson: TraitHelpers.deserializeOptionStr(deserializer),
      passphrase: deserializer.deserializeString(),
      bip39AccountIndex: deserializer.deserializeUint32(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static RestoreWalletDataRequest bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = RestoreWalletDataRequest.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String seed;
  final String network;
  final List<OwnedOutput> outputs;
  final Uint64 daemonHeight;
  final Uint64 currentHeight;
  final String? blockHashesJson;
  final String? pendingStateJson;
  final String passphrase;
  final int bip39AccountIndex;

  RestoreWalletDataRequest copyWith({
    String? seed,
    String? network,
    List<OwnedOutput>? outputs,
    Uint64? daemonHeight,
    Uint64? currentHeight,
    String? Function()? blockHashesJson,
    String? Function()? pendingStateJson,
    String? passphrase,
    int? bip39AccountIndex,
  }) {
    return RestoreWalletDataRequest(
      seed: seed ?? this.seed,
      network: network ?? this.network,
      outputs: outputs ?? this.outputs,
      daemonHeight: daemonHeight ?? this.daemonHeight,
      currentHeight: currentHeight ?? this.currentHeight,
      blockHashesJson: blockHashesJson == null ? this.blockHashesJson : blockHashesJson(),
      pendingStateJson: pendingStateJson == null ? this.pendingStateJson : pendingStateJson(),
      passphrase: passphrase ?? this.passphrase,
      bip39AccountIndex: bip39AccountIndex ?? this.bip39AccountIndex,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(seed);
    serializer.serializeString(network);
    TraitHelpers.serializeVectorOwnedOutput(outputs, serializer);
    serializer.serializeUint64(daemonHeight);
    serializer.serializeUint64(currentHeight);
    TraitHelpers.serializeOptionStr(blockHashesJson, serializer);
    TraitHelpers.serializeOptionStr(pendingStateJson, serializer);
    serializer.serializeString(passphrase);
    serializer.serializeUint32(bip39AccountIndex);
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

    return other is RestoreWalletDataRequest
      && seed == other.seed
      && network == other.network
      && listEquals(outputs, other.outputs)
      && daemonHeight == other.daemonHeight
      && currentHeight == other.currentHeight
      && blockHashesJson == other.blockHashesJson
      && pendingStateJson == other.pendingStateJson
      && passphrase == other.passphrase
      && bip39AccountIndex == other.bip39AccountIndex;
  }

  @override
  int get hashCode => Object.hash(
        seed,
        network,
        outputs,
        daemonHeight,
        currentHeight,
        blockHashesJson,
        pendingStateJson,
        passphrase,
        bip39AccountIndex,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'seed: $seed, '
        'network: $network, '
        'outputs: $outputs, '
        'daemonHeight: $daemonHeight, '
        'currentHeight: $currentHeight, '
        'blockHashesJson: $blockHashesJson, '
        'pendingStateJson: $pendingStateJson, '
        'passphrase: $passphrase, '
        'bip39AccountIndex: $bip39AccountIndex'
        ')';
      return true;
    }());

    return fullString ?? 'RestoreWalletDataRequest';
  }
}

extension RestoreWalletDataRequestDartSignalExt on RestoreWalletDataRequest {
  /// Sends the signal to Rust.
  /// Passing data from Rust to Dart involves a memory copy
  /// because Rust cannot own data managed by Dart's garbage collector.
  void sendSignalToRust() {
    final messageBytes = bincodeSerialize();
    final binary = Uint8List(0);
    sendDartSignal(
      'rinf_send_dart_signal_restore_wallet_data_request',
      messageBytes,
      binary,
    );
  }
}
