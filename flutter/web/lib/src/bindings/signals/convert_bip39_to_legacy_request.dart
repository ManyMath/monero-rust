// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class ConvertBip39ToLegacyRequest {
  const ConvertBip39ToLegacyRequest({
    required this.bip39Mnemonic,
    required this.accountIndex,
  });

  static ConvertBip39ToLegacyRequest deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = ConvertBip39ToLegacyRequest(
      bip39Mnemonic: deserializer.deserializeString(),
      accountIndex: deserializer.deserializeUint32(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static ConvertBip39ToLegacyRequest bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = ConvertBip39ToLegacyRequest.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String bip39Mnemonic;
  final int accountIndex;

  ConvertBip39ToLegacyRequest copyWith({
    String? bip39Mnemonic,
    int? accountIndex,
  }) {
    return ConvertBip39ToLegacyRequest(
      bip39Mnemonic: bip39Mnemonic ?? this.bip39Mnemonic,
      accountIndex: accountIndex ?? this.accountIndex,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(bip39Mnemonic);
    serializer.serializeUint32(accountIndex);
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

    return other is ConvertBip39ToLegacyRequest
      && bip39Mnemonic == other.bip39Mnemonic
      && accountIndex == other.accountIndex;
  }

  @override
  int get hashCode => Object.hash(
        bip39Mnemonic,
        accountIndex,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'bip39Mnemonic: $bip39Mnemonic, '
        'accountIndex: $accountIndex'
        ')';
      return true;
    }());

    return fullString ?? 'ConvertBip39ToLegacyRequest';
  }
}

extension ConvertBip39ToLegacyRequestDartSignalExt on ConvertBip39ToLegacyRequest {
  void sendSignalToRust() {
    final messageBytes = bincodeSerialize();
    final binary = Uint8List(0);
    sendDartSignal(
      'rinf_send_dart_signal_convert_bip39_to_legacy_request',
      messageBytes,
      binary,
    );
  }
}
