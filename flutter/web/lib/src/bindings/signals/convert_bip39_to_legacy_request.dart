// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class ConvertBip39ToLegacyRequest {
  const ConvertBip39ToLegacyRequest({
    required this.bip39Mnemonic,
    required this.accountIndex,
    required this.passphrase,
  });

  static ConvertBip39ToLegacyRequest deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = ConvertBip39ToLegacyRequest(
      bip39Mnemonic: deserializer.deserializeString(),
      accountIndex: deserializer.deserializeUint32(),
      passphrase: deserializer.deserializeString(),
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
  final String passphrase;

  ConvertBip39ToLegacyRequest copyWith({
    String? bip39Mnemonic,
    int? accountIndex,
    String? passphrase,
  }) {
    return ConvertBip39ToLegacyRequest(
      bip39Mnemonic: bip39Mnemonic ?? this.bip39Mnemonic,
      accountIndex: accountIndex ?? this.accountIndex,
      passphrase: passphrase ?? this.passphrase,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(bip39Mnemonic);
    serializer.serializeUint32(accountIndex);
    serializer.serializeString(passphrase);
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
      && accountIndex == other.accountIndex
      && passphrase == other.passphrase;
  }

  @override
  int get hashCode => Object.hash(
        bip39Mnemonic,
        accountIndex,
        passphrase,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'bip39Mnemonic: $bip39Mnemonic, '
        'accountIndex: $accountIndex, '
        'passphrase: $passphrase'
        ')';
      return true;
    }());

    return fullString ?? 'ConvertBip39ToLegacyRequest';
  }
}

extension ConvertBip39ToLegacyRequestDartSignalExt on ConvertBip39ToLegacyRequest {
  /// Sends the signal to Rust.
  /// Passing data from Rust to Dart involves a memory copy
  /// because Rust cannot own data managed by Dart's garbage collector.
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
