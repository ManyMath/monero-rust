import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

class OutputUtils {
  /// Select all spendable outputs (confirmed and unspent)
  static Set<String> selectAllSpendable(
    List<OwnedOutput> allOutputs,
    int currentHeight,
  ) {
    final selected = <String>{};
    for (var output in allOutputs) {
      if (output.spent) continue;
      final outputHeight = output.blockHeight.toInt();
      final confirmations = outputHeight > 0 ? currentHeight - outputHeight : 0;
      if (confirmations >= 10) {
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
      if (output.spent) continue;
      final outputHeight = output.blockHeight.toInt();
      final confirmations = outputHeight > 0 ? currentHeight - outputHeight : 0;
      if (confirmations < 10) continue;
      final outputKey = '${output.txHash}:${output.outputIndex}';
      if (selectedOutputs.contains(outputKey)) {
        total += output.amount.toInt();
      }
    }
    return total;
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
}
