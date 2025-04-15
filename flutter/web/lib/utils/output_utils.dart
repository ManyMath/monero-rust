import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import 'output_lock_utils.dart';

class OutputUtils {
  /// Select all spendable outputs (confirmed and unspent)
  static Set<String> selectAllSpendable(
    List<OwnedOutput> allOutputs,
    int currentHeight,
  ) {
    final selected = <String>{};
    for (var output in allOutputs) {
      if (OutputLockUtils.isOutputSpendable(
        output: output,
        currentHeight: currentHeight,
      )) {
        final outputKey = '${output.txHash}:${output.outputIndex}';
        selected.add(outputKey);
      }
    }
    return selected;
  }

  /// Calculate total atomic units of selected outputs
  static int getSelectedOutputsTotal(
    List<OwnedOutput> allOutputs,
    Set<String> selectedOutputs,
    int currentHeight,
  ) {
    int total = 0;
    for (var output in allOutputs) {
      final outputKey = '${output.txHash}:${output.outputIndex}';
      if (selectedOutputs.contains(outputKey)) {
        if (OutputLockUtils.isOutputSpendable(
          output: output,
          currentHeight: currentHeight,
        )) {
          total += output.amount.toInt();
        }
      }
    }
    return total;
  }

  /// Create a copy of the output with spent set to true
  static OwnedOutput markAsSpent(OwnedOutput output) {
    return output.copyWith(spent: true);
  }

  /// Calculate total XMR from recipient amount controllers
  static double getRecipientsTotal(List<TextEditingController> amountControllers) {
    double total = 0;
    for (var controller in amountControllers) {
      final amount = double.tryParse(controller.text.trim());
      if (amount != null && amount > 0) {
        total += amount;
      }
    }
    return total;
  }

  /// Merge scanned outputs into existing list in-place.
  /// Adds new outputs; updates unconfirmed (blockHeight=0) with confirmed version.
  static void mergeScannedOutputs(
    List<OwnedOutput> existing,
    List<OwnedOutput> incoming,
  ) {
    print('DEBUG mergeScannedOutputs: START');
    print('  - Existing outputs count: ${existing.length}');
    print('  - Incoming outputs count: ${incoming.length}');

    for (var output in incoming) {
      final accountIdx = output.subaddressIndex?.item1 ?? 0;
      print('  - Processing output: txHash=${output.txHash.length > 8 ? output.txHash.substring(0, 8) : output.txHash}..., outputIndex=${output.outputIndex}, account=$accountIdx, amount=${output.amountXmr}');

      final idx = existing.indexWhere((o) =>
        o.txHash == output.txHash && o.outputIndex == output.outputIndex
      );
      if (idx == -1) {
        existing.add(output);
        print('    → ADDED new output (now ${existing.length} total)');
      } else if (existing[idx].blockHeight.toInt() == 0) {
        existing[idx] = output;
        print('    → UPDATED unconfirmed output at index $idx');
      } else {
        print('    → SKIPPED (already exists at index $idx)');
      }
    }

    print('  - Final existing outputs count: ${existing.length}');
    print('DEBUG mergeScannedOutputs: END');
  }

  /// Add outputs not already present (by txHash+outputIndex) in-place.
  static void addIfAbsent(
    List<OwnedOutput> existing,
    List<OwnedOutput> incoming,
  ) {
    final existingKeys = <String>{
      for (var o in existing) '${o.txHash}:${o.outputIndex}',
    };
    for (var output in incoming) {
      final key = '${output.txHash}:${output.outputIndex}';
      if (existingKeys.add(key)) {
        existing.add(output);
      }
    }
  }

  /// Convert a ChangeOutput to an unconfirmed OwnedOutput.
  static OwnedOutput changeOutputToOwned(ChangeOutput change) {
    return OwnedOutput(
      txHash: change.txHash,
      outputIndex: change.outputIndex,
      amount: change.amount,
      amountXmr: change.amountXmr,
      key: change.key,
      keyOffset: change.keyOffset,
      commitmentMask: change.commitmentMask,
      subaddressIndex: change.subaddressIndex,
      paymentId: null,
      receivedOutputBytes: change.receivedOutputBytes,
      blockHeight: Uint64(BigInt.zero),
      spent: false,
      keyImage: change.keyImage,
      isCoinbase: false, // Change outputs are never coinbase
    );
  }

  /// Mark outputs as spent by matching key images; removes from selectedOutputs.
  static void markSpentByKeyImages(
    List<OwnedOutput> outputs,
    List<String> keyImages,
    Set<String> selectedOutputs,
  ) {
    final keyImageSet = keyImages.toSet();
    for (int i = 0; i < outputs.length; i++) {
      if (keyImageSet.contains(outputs[i].keyImage) &&
          !outputs[i].spent) {
        final outputKey = '${outputs[i].txHash}:${outputs[i].outputIndex}';
        selectedOutputs.remove(outputKey);
        outputs[i] = markAsSpent(outputs[i]);
      }
    }
  }

  /// Mark outputs as spent by "txHash:outputIndex" keys; removes from selectedOutputs.
  static void markSpentByOutputKeys(
    List<OwnedOutput> outputs,
    List<String> outputKeys,
    Set<String> selectedOutputs,
  ) {
    final keySet = outputKeys.toSet();
    for (int i = 0; i < outputs.length; i++) {
      final key = '${outputs[i].txHash}:${outputs[i].outputIndex}';
      if (keySet.contains(key)) {
        selectedOutputs.remove(key);
        outputs[i] = markAsSpent(outputs[i]);
      }
    }
  }
}
