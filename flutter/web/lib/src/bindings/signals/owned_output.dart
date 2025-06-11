// ignore_for_file: type=lint, type=warning
part of 'signals.dart';


@immutable
class OwnedOutput {
  const OwnedOutput({
    required this.txHash,
    required this.outputIndex,
    required this.amount,
    required this.amountXmr,
    required this.key,
    required this.keyOffset,
    required this.commitmentMask,
    this.subaddressIndex,
    this.paymentId,
    required this.receivedOutputBytes,
    required this.blockHeight,
    required this.spent,
    required this.keyImage,
    required this.isCoinbase,
    required this.frozen,
  });

  static OwnedOutput deserialize(BinaryDeserializer deserializer) {
    deserializer.increaseContainerDepth();
    final instance = OwnedOutput(
      txHash: deserializer.deserializeString(),
      outputIndex: deserializer.deserializeUint8(),
      amount: deserializer.deserializeUint64(),
      amountXmr: deserializer.deserializeString(),
      key: deserializer.deserializeString(),
      keyOffset: deserializer.deserializeString(),
      commitmentMask: deserializer.deserializeString(),
      subaddressIndex: TraitHelpers.deserializeOptionTuple2U32U32(deserializer),
      paymentId: TraitHelpers.deserializeOptionStr(deserializer),
      receivedOutputBytes: deserializer.deserializeString(),
      blockHeight: deserializer.deserializeUint64(),
      spent: deserializer.deserializeBool(),
      keyImage: deserializer.deserializeString(),
      isCoinbase: deserializer.deserializeBool(),
      frozen: deserializer.deserializeBool(),
    );
    deserializer.decreaseContainerDepth();
    return instance;
  }

  static OwnedOutput bincodeDeserialize(Uint8List input) {
    final deserializer = BincodeDeserializer(input);
    final value = OwnedOutput.deserialize(deserializer);
    if (deserializer.offset < input.length) {
      throw Exception('Some input bytes were not read');
    }
    return value;
  }

  final String txHash;
  final int outputIndex;
  final Uint64 amount;
  final String amountXmr;
  final String key;
  final String keyOffset;
  final String commitmentMask;
  final Tuple2<int, int>? subaddressIndex;
  final String? paymentId;
  final String receivedOutputBytes;
  final Uint64 blockHeight;
  final bool spent;
  final String keyImage;
  final bool isCoinbase;
  final bool frozen;

  OwnedOutput copyWith({
    String? txHash,
    int? outputIndex,
    Uint64? amount,
    String? amountXmr,
    String? key,
    String? keyOffset,
    String? commitmentMask,
    Tuple2<int, int>? Function()? subaddressIndex,
    String? Function()? paymentId,
    String? receivedOutputBytes,
    Uint64? blockHeight,
    bool? spent,
    String? keyImage,
    bool? isCoinbase,
    bool? frozen,
  }) {
    return OwnedOutput(
      txHash: txHash ?? this.txHash,
      outputIndex: outputIndex ?? this.outputIndex,
      amount: amount ?? this.amount,
      amountXmr: amountXmr ?? this.amountXmr,
      key: key ?? this.key,
      keyOffset: keyOffset ?? this.keyOffset,
      commitmentMask: commitmentMask ?? this.commitmentMask,
      subaddressIndex: subaddressIndex == null ? this.subaddressIndex : subaddressIndex(),
      paymentId: paymentId == null ? this.paymentId : paymentId(),
      receivedOutputBytes: receivedOutputBytes ?? this.receivedOutputBytes,
      blockHeight: blockHeight ?? this.blockHeight,
      spent: spent ?? this.spent,
      keyImage: keyImage ?? this.keyImage,
      isCoinbase: isCoinbase ?? this.isCoinbase,
      frozen: frozen ?? this.frozen,
    );
  }

  void serialize(BinarySerializer serializer) {
    serializer.increaseContainerDepth();
    serializer.serializeString(txHash);
    serializer.serializeUint8(outputIndex);
    serializer.serializeUint64(amount);
    serializer.serializeString(amountXmr);
    serializer.serializeString(key);
    serializer.serializeString(keyOffset);
    serializer.serializeString(commitmentMask);
    TraitHelpers.serializeOptionTuple2U32U32(subaddressIndex, serializer);
    TraitHelpers.serializeOptionStr(paymentId, serializer);
    serializer.serializeString(receivedOutputBytes);
    serializer.serializeUint64(blockHeight);
    serializer.serializeBool(spent);
    serializer.serializeString(keyImage);
    serializer.serializeBool(isCoinbase);
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

    return other is OwnedOutput
      && txHash == other.txHash
      && outputIndex == other.outputIndex
      && amount == other.amount
      && amountXmr == other.amountXmr
      && key == other.key
      && keyOffset == other.keyOffset
      && commitmentMask == other.commitmentMask
      && subaddressIndex == other.subaddressIndex
      && paymentId == other.paymentId
      && receivedOutputBytes == other.receivedOutputBytes
      && blockHeight == other.blockHeight
      && spent == other.spent
      && keyImage == other.keyImage
      && isCoinbase == other.isCoinbase
      && frozen == other.frozen;
  }

  @override
  int get hashCode => Object.hash(
        txHash,
        outputIndex,
        amount,
        amountXmr,
        key,
        keyOffset,
        commitmentMask,
        subaddressIndex,
        paymentId,
        receivedOutputBytes,
        blockHeight,
        spent,
        keyImage,
        isCoinbase,
        frozen,
      );

  @override
  String toString() {
    String? fullString;

    assert(() {
      fullString = '$runtimeType('
        'txHash: $txHash, '
        'outputIndex: $outputIndex, '
        'amount: $amount, '
        'amountXmr: $amountXmr, '
        'key: $key, '
        'keyOffset: $keyOffset, '
        'commitmentMask: $commitmentMask, '
        'subaddressIndex: $subaddressIndex, '
        'paymentId: $paymentId, '
        'receivedOutputBytes: $receivedOutputBytes, '
        'blockHeight: $blockHeight, '
        'spent: $spent, '
        'keyImage: $keyImage, '
        'isCoinbase: $isCoinbase, '
        'frozen: $frozen'
        ')';
      return true;
    }());

    return fullString ?? 'OwnedOutput';
  }
}
