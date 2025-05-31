// ignore_for_file: type=lint, type=warning
part of 'signals.dart';
class TraitHelpers {
  static void serializeOptionStr(String? value, BinarySerializer serializer) {
    if (value == null) {
        serializer.serializeOptionTag(false);
    } else {
        serializer.serializeOptionTag(true);
        serializer.serializeString(value);
    }
  }

  static String? deserializeOptionStr(BinaryDeserializer deserializer) {
    final tag = deserializer.deserializeOptionTag();
    if (tag) {
        return deserializer.deserializeString();
    } else {
        return null;
    }
  }

  static void serializeOptionTuple2U32U32(Tuple2<int, int>? value, BinarySerializer serializer) {
    if (value == null) {
        serializer.serializeOptionTag(false);
    } else {
        serializer.serializeOptionTag(true);
        TraitHelpers.serializeTuple2U32U32(value, serializer);
    }
  }

  static Tuple2<int, int>? deserializeOptionTuple2U32U32(BinaryDeserializer deserializer) {
    final tag = deserializer.deserializeOptionTag();
    if (tag) {
        return TraitHelpers.deserializeTuple2U32U32(deserializer);
    } else {
        return null;
    }
  }

  static void serializeOptionU64(Uint64? value, BinarySerializer serializer) {
    if (value == null) {
        serializer.serializeOptionTag(false);
    } else {
        serializer.serializeOptionTag(true);
        serializer.serializeUint64(value);
    }
  }

  static Uint64? deserializeOptionU64(BinaryDeserializer deserializer) {
    final tag = deserializer.deserializeOptionTag();
    if (tag) {
        return deserializer.deserializeUint64();
    } else {
        return null;
    }
  }

  static void serializeOptionVectorStr(List<String>? value, BinarySerializer serializer) {
    if (value == null) {
        serializer.serializeOptionTag(false);
    } else {
        serializer.serializeOptionTag(true);
        TraitHelpers.serializeVectorStr(value, serializer);
    }
  }

  static List<String>? deserializeOptionVectorStr(BinaryDeserializer deserializer) {
    final tag = deserializer.deserializeOptionTag();
    if (tag) {
        return TraitHelpers.deserializeVectorStr(deserializer);
    } else {
        return null;
    }
  }

  static void serializeOptionVectorU32(List<int>? value, BinarySerializer serializer) {
    if (value == null) {
        serializer.serializeOptionTag(false);
    } else {
        serializer.serializeOptionTag(true);
        TraitHelpers.serializeVectorU32(value, serializer);
    }
  }

  static List<int>? deserializeOptionVectorU32(BinaryDeserializer deserializer) {
    final tag = deserializer.deserializeOptionTag();
    if (tag) {
        return TraitHelpers.deserializeVectorU32(deserializer);
    } else {
        return null;
    }
  }

  static void serializeTuple2U32U32(Tuple2<int, int> value, BinarySerializer serializer) {
    serializer.serializeUint32(value.item1);
    serializer.serializeUint32(value.item2);
  }

  static Tuple2<int, int> deserializeTuple2U32U32(BinaryDeserializer deserializer) {
    return Tuple2<int, int>(
        deserializer.deserializeUint32(),
        deserializer.deserializeUint32()
    );
  }

  static void serializeVectorChangeOutput(List<ChangeOutput> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        item.serialize(serializer);
    }
  }

  static List<ChangeOutput> deserializeVectorChangeOutput(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => ChangeOutput.deserialize(deserializer));
  }

  static void serializeVectorDoubleSpendConflict(List<DoubleSpendConflict> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        item.serialize(serializer);
    }
  }

  static List<DoubleSpendConflict> deserializeVectorDoubleSpendConflict(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => DoubleSpendConflict.deserialize(deserializer));
  }

  static void serializeVectorOwnedOutput(List<OwnedOutput> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        item.serialize(serializer);
    }
  }

  static List<OwnedOutput> deserializeVectorOwnedOutput(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => OwnedOutput.deserialize(deserializer));
  }

  static void serializeVectorRecipient(List<Recipient> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        item.serialize(serializer);
    }
  }

  static List<Recipient> deserializeVectorRecipient(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => Recipient.deserialize(deserializer));
  }

  static void serializeVectorWalletConfig(List<WalletConfig> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        item.serialize(serializer);
    }
  }

  static List<WalletConfig> deserializeVectorWalletConfig(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => WalletConfig.deserialize(deserializer));
  }

  static void serializeVectorWalletScanResult(List<WalletScanResult> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        item.serialize(serializer);
    }
  }

  static List<WalletScanResult> deserializeVectorWalletScanResult(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => WalletScanResult.deserialize(deserializer));
  }

  static void serializeVectorStr(List<String> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        serializer.serializeString(item);
    }
  }

  static List<String> deserializeVectorStr(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => deserializer.deserializeString());
  }

  static void serializeVectorU32(List<int> value, BinarySerializer serializer) {
    serializer.serializeLength(value.length);
    for (final item in value) {
        serializer.serializeUint32(item);
    }
  }

  static List<int> deserializeVectorU32(BinaryDeserializer deserializer) {
    final length = deserializer.deserializeLength();
    return List.generate(length, (_) => deserializer.deserializeUint32());
  }

}

