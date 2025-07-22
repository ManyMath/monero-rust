import '../src/ffi/signal_types.dart';

class WalletTransaction {
  final String txHash;
  final int blockHeight;
  final int blockTimestamp;
  final List<OwnedOutput> receivedOutputs;
  final List<String> spentKeyImages;
  String? description;

  WalletTransaction({
    required this.txHash,
    required this.blockHeight,
    required this.blockTimestamp,
    required this.receivedOutputs,
    required this.spentKeyImages,
    this.description,
  });

  double balanceChange(Map<String, OwnedOutput> keyImageMap) {
    int receivedAtomic = 0;
    for (var output in receivedOutputs) {
      receivedAtomic += output.amount;
    }
    int spentAtomic = 0;
    for (var keyImage in spentKeyImages) {
      final spentOutput = keyImageMap[keyImage];
      if (spentOutput != null) {
        spentAtomic += spentOutput.amount;
      }
    }
    return (receivedAtomic - spentAtomic) / 1e12;
  }

  bool isIncoming(Map<String, OwnedOutput> keyImageMap) => balanceChange(keyImageMap) > 0;

  Map<String, dynamic> toJson() => {
    'txHash': txHash,
    'blockHeight': blockHeight,
    'blockTimestamp': blockTimestamp,
    'receivedOutputs': receivedOutputs.map((o) => {
      'txHash': o.txHash,
      'outputIndex': o.outputIndex,
      'amount': o.amount.toString(),
      'amountXmr': o.amountXmr,
      'key': o.key,
      'keyOffset': o.keyOffset,
      'commitmentMask': o.commitmentMask,
      'subaddressIndex': o.subaddressIndex != null
          ? [o.subaddressIndex!.$1, o.subaddressIndex!.$2]
          : null,
      'paymentId': o.paymentId,
      'receivedOutputBytes': o.receivedOutputBytes,
      'blockHeight': o.blockHeight.toString(),
      'spent': o.spent,
      'keyImage': o.keyImage,
      'frozen': o.frozen,
    }).toList(),
    'spentKeyImages': spentKeyImages,
    if (description != null) 'description': description,
  };

  factory WalletTransaction.fromJsonCompact(
    Map<String, dynamic> json,
    Map<String, OwnedOutput> outputLookup,
  ) {
    final refs = (json['receivedOutputRefs'] as List).cast<String>();
    final outputs = refs
        .map((ref) => outputLookup[ref])
        .where((o) => o != null)
        .cast<OwnedOutput>()
        .toList();

    return WalletTransaction(
      txHash: json['txHash'] as String,
      blockHeight: json['blockHeight'] as int,
      blockTimestamp: json['blockTimestamp'] as int,
      receivedOutputs: outputs,
      spentKeyImages: (json['spentKeyImages'] as List).cast<String>(),
      description: json['description'] as String?,
    );
  }

  WalletTransaction copyWith({
    String? txHash,
    int? blockHeight,
    int? blockTimestamp,
    List<OwnedOutput>? receivedOutputs,
    List<String>? spentKeyImages,
    String? description,
  }) {
    return WalletTransaction(
      txHash: txHash ?? this.txHash,
      blockHeight: blockHeight ?? this.blockHeight,
      blockTimestamp: blockTimestamp ?? this.blockTimestamp,
      receivedOutputs: receivedOutputs ?? this.receivedOutputs,
      spentKeyImages: spentKeyImages ?? this.spentKeyImages,
      description: description ?? this.description,
    );
  }

  factory WalletTransaction.fromJson(Map<String, dynamic> json) {
    return WalletTransaction(
      txHash: json['txHash'] as String,
      blockHeight: json['blockHeight'] as int,
      blockTimestamp: json['blockTimestamp'] as int,
      receivedOutputs: (json['receivedOutputs'] as List).map((o) {
        final outputData = o as Map<String, dynamic>;
        return OwnedOutput(
          txHash: outputData['txHash'] as String,
          outputIndex: outputData['outputIndex'] as int,
          amount: int.parse(outputData['amount'] as String),
          amountXmr: outputData['amountXmr'] as String,
          key: outputData['key'] as String,
          keyOffset: outputData['keyOffset'] as String,
          commitmentMask: outputData['commitmentMask'] as String,
          subaddressIndex: outputData['subaddressIndex'] != null
              ? (
                  outputData['subaddressIndex'][0] as int,
                  outputData['subaddressIndex'][1] as int,
                )
              : null,
          paymentId: outputData['paymentId'] as String?,
          receivedOutputBytes: outputData['receivedOutputBytes'] as String,
          blockHeight: int.parse(outputData['blockHeight'] as String),
          // Handle backward compatibility: these fields were added later
          spent: outputData.containsKey('spent') && outputData['spent'] != null
              ? outputData['spent'] as bool
              : false,  // Default to unspent for backward compatibility
          keyImage: outputData['keyImage'] as String,
          isCoinbase: outputData.containsKey('isCoinbase') && outputData['isCoinbase'] != null
              ? outputData['isCoinbase'] as bool
              : false,  // Default to non-coinbase for backward compatibility
          frozen: outputData.containsKey('frozen') && outputData['frozen'] != null
              ? outputData['frozen'] as bool
              : false,
        );
      }).toList(),
      spentKeyImages: (json['spentKeyImages'] as List).cast<String>(),
      description: json['description'] as String?,
    );
  }
}
