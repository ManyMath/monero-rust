import 'package:tuple/tuple.dart';
import '../src/bindings/bindings.dart';

class WalletTransaction {
  final String txHash;
  final int blockHeight;
  final int blockTimestamp;
  final List<OwnedOutput> receivedOutputs;
  final List<String> spentKeyImages;

  WalletTransaction({
    required this.txHash,
    required this.blockHeight,
    required this.blockTimestamp,
    required this.receivedOutputs,
    required this.spentKeyImages,
  });

  double balanceChange(Map<String, OwnedOutput> keyImageMap) {
    double received = 0;
    for (var output in receivedOutputs) {
      received += double.tryParse(output.amountXmr) ?? 0;
    }

    double spent = 0;
    for (var keyImage in spentKeyImages) {
      final spentOutput = keyImageMap[keyImage];
      if (spentOutput != null) {
        spent += double.tryParse(spentOutput.amountXmr) ?? 0;
      }
    }

    return received - spent;
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
          ? [o.subaddressIndex!.item1, o.subaddressIndex!.item2]
          : null,
      'paymentId': o.paymentId,
      'receivedOutputBytes': o.receivedOutputBytes,
      'blockHeight': o.blockHeight.toString(),
      'spent': o.spent,
      'keyImage': o.keyImage,
    }).toList(),
    'spentKeyImages': spentKeyImages,
  };

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
          amount: Uint64(BigInt.parse(outputData['amount'] as String)),
          amountXmr: outputData['amountXmr'] as String,
          key: outputData['key'] as String,
          keyOffset: outputData['keyOffset'] as String,
          commitmentMask: outputData['commitmentMask'] as String,
          subaddressIndex: outputData['subaddressIndex'] != null
              ? Tuple2<int, int>(
                  outputData['subaddressIndex'][0] as int,
                  outputData['subaddressIndex'][1] as int,
                )
              : null,
          paymentId: outputData['paymentId'] as String?,
          receivedOutputBytes: outputData['receivedOutputBytes'] as String,
          blockHeight: Uint64(BigInt.parse(outputData['blockHeight'] as String)),
          // Handle backward compatibility: these fields were added later
          spent: outputData.containsKey('spent') && outputData['spent'] != null
              ? outputData['spent'] as bool
              : false,  // Default to unspent for backward compatibility
          keyImage: outputData['keyImage'] as String,
          isCoinbase: outputData.containsKey('isCoinbase') && outputData['isCoinbase'] != null
              ? outputData['isCoinbase'] as bool
              : false,  // Default to non-coinbase for backward compatibility
        );
      }).toList(),
      spentKeyImages: (json['spentKeyImages'] as List).cast<String>(),
    );
  }
}
