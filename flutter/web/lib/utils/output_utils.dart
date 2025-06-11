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
    // Build O(1) lookup: "txHash:outputIndex" → index in existing list
    final indexMap = <String, int>{};
    for (int i = 0; i < existing.length; i++) {
      indexMap['${existing[i].txHash}:${existing[i].outputIndex}'] = i;
    }

    for (var output in incoming) {
      final key = '${output.txHash}:${output.outputIndex}';
      final idx = indexMap[key];
      if (idx == null) {
        indexMap[key] = existing.length;
        existing.add(output);
      } else if (existing[idx].blockHeight.toInt() == 0) {
        existing[idx] = output;
      }
    }
  }

  /// Add outputs not already present (by txHash+outputIndex) in-place.
  /// If an existing output has blockHeight=0 and the incoming one has a
  /// non-zero blockHeight, update the existing entry (mempool→confirmed).
  static void addIfAbsent(
    List<OwnedOutput> existing,
    List<OwnedOutput> incoming,
  ) {
    final indexMap = <String, int>{};
    for (int i = 0; i < existing.length; i++) {
      indexMap['${existing[i].txHash}:${existing[i].outputIndex}'] = i;
    }
    for (var output in incoming) {
      final key = '${output.txHash}:${output.outputIndex}';
      final idx = indexMap[key];
      if (idx == null) {
        indexMap[key] = existing.length;
        existing.add(output);
      } else if (existing[idx].blockHeight.toInt() == 0 &&
          output.blockHeight.toInt() != 0) {
        existing[idx] = output;
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
      frozen: false,
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
