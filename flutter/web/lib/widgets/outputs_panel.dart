import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import 'common_widgets.dart';

/// Widget that displays UTXO/coin management interface.
///
/// Shows output list with filtering, coin selection checkboxes,
/// sort controls, and balance summaries.
class OutputsPanel extends StatelessWidget {
  final List<OwnedOutput> allOutputs;
  final int currentHeight;
  final bool showSpentOutputs;
  final String sortBy;
  final bool sortAscending;
  final Set<String> selectedOutputs;
  final VoidCallback onToggleShowSpent;
  final VoidCallback onSelectAllSpendable;
  final VoidCallback onClearSelection;
  final Function(String sortKey) onSortChanged;
  final Function(String outputKey, bool selected) onOutputSelectionChanged;

  const OutputsPanel({
    super.key,
    required this.allOutputs,
    required this.currentHeight,
    required this.showSpentOutputs,
    required this.sortBy,
    required this.sortAscending,
    required this.selectedOutputs,
    required this.onToggleShowSpent,
    required this.onSelectAllSpendable,
    required this.onClearSelection,
    required this.onSortChanged,
    required this.onOutputSelectionChanged,
  });

  List<OwnedOutput> _sortedOutputs() {
    final outputs = showSpentOutputs
        ? allOutputs
        : allOutputs.where((o) => !o.spent).toList();

    outputs.sort((a, b) {
      int comparison;
      if (sortBy == 'confirms') {
        final aHeight = a.blockHeight.toInt();
        final bHeight = b.blockHeight.toInt();
        final aConfirms = aHeight > 0 ? currentHeight - aHeight : 0;
        final bConfirms = bHeight > 0 ? currentHeight - bHeight : 0;
        comparison = aConfirms.compareTo(bConfirms);
      } else {
        final aValue = double.tryParse(a.amountXmr) ?? 0;
        final bValue = double.tryParse(b.amountXmr) ?? 0;
        comparison = aValue.compareTo(bValue);
      }
      return sortAscending ? comparison : -comparison;
    });

    return outputs;
  }

  @override
  Widget build(BuildContext context) {
    if (allOutputs.isEmpty) {
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(16.0),
          child: Text(
            'No outputs found. Scan blocks to find outputs.',
            style: TextStyle(color: Colors.grey),
          ),
        ),
      );
    }

    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Row(
              children: [
                if (allOutputs.any((o) => o.spent)) ...[
                  Checkbox(
                    value: showSpentOutputs,
                    onChanged: (value) => onToggleShowSpent(),
                  ),
                  GestureDetector(
                    onTap: onToggleShowSpent,
                    child: const Text('Show spent'),
                  ),
                  const SizedBox(width: 12),
                ],
                const Text('Select: ', style: TextStyle(fontSize: 12)),
                CommonWidgets.buildSelectButton(label: 'All', onPressed: onSelectAllSpendable),
                const SizedBox(width: 4),
                CommonWidgets.buildSelectButton(label: 'None', onPressed: onClearSelection),
                const Spacer(),
                const Text('Sort: ', style: TextStyle(fontSize: 12)),
                CommonWidgets.buildSortButton(
                  label: 'Confirms',
                  sortKey: 'confirms',
                  currentSortKey: sortBy,
                  isAscending: sortAscending,
                  onTap: () => onSortChanged('confirms'),
                ),
                const SizedBox(width: 4),
                CommonWidgets.buildSortButton(
                  label: 'Value',
                  sortKey: 'value',
                  currentSortKey: sortBy,
                  isAscending: sortAscending,
                  onTap: () => onSortChanged('value'),
                ),
              ],
            ),
          ),
          ..._sortedOutputs().map((output) {
            final outputHeight = output.blockHeight.toInt();
            final confirmations = outputHeight > 0
                ? currentHeight - outputHeight
                : 0;
            final isSpendable = confirmations >= 10 && !output.spent;
            final statusColor = output.spent
                ? Colors.grey
                : isSpendable
                    ? Colors.green
                    : Colors.orange;
            final statusText = output.spent
                ? 'SPENT'
                : isSpendable
                    ? 'SPENDABLE'
                    : 'LOCKED ($confirmations/10)';

            final outputKey = '${output.txHash}:${output.outputIndex}';
            final isSelected = selectedOutputs.contains(outputKey);

            return Card(
              margin: const EdgeInsets.only(bottom: 12),
              elevation: 2,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        Row(
                          children: [
                            if (isSpendable)
                              SizedBox(
                                width: 24,
                                height: 24,
                                child: Checkbox(
                                  value: isSelected,
                                  onChanged: (value) {
                                    onOutputSelectionChanged(outputKey, value ?? false);
                                  },
                                ),
                              ),
                            if (isSpendable) const SizedBox(width: 8),
                            Text(
                              '${output.amountXmr} XMR',
                              style: TextStyle(
                                fontWeight: FontWeight.bold,
                                fontSize: 16,
                                color: output.spent ? Colors.grey.shade600 : Colors.black,
                              ),
                            ),
                          ],
                        ),
                        Container(
                          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                          decoration: BoxDecoration(
                            color: statusColor.withOpacity(0.1),
                            borderRadius: BorderRadius.circular(4),
                            border: Border.all(color: statusColor),
                          ),
                          child: Text(
                            statusText,
                            style: TextStyle(
                              color: statusColor.shade800,
                              fontSize: 10,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    CommonWidgets.buildOutputDetailRow(label: 'TX Hash', value: output.txHash, mono: true),
                    CommonWidgets.buildOutputDetailRow(label: 'Output Index', value: '${output.outputIndex}'),
                    CommonWidgets.buildOutputDetailRow(label: 'Block Height', value: '$outputHeight'),
                    if (output.subaddressIndex != null)
                      CommonWidgets.buildOutputDetailRow(
                        label: 'Subaddress',
                        value: '${output.subaddressIndex!.item1}/${output.subaddressIndex!.item2}',
                      ),
                    if (output.paymentId != null)
                      CommonWidgets.buildOutputDetailRow(label: 'Payment ID', value: output.paymentId!, mono: true),
                  ],
                ),
              ),
            );
          }),
        ],
      ),
    );
  }
}
