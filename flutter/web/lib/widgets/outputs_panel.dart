import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../utils/output_lock_utils.dart';
import 'common_widgets.dart';

/// Widget that displays UTXO/coin management interface.
///
/// Shows output list with filtering, freeze/thaw checkboxes,
/// sort controls, and balance summaries.
class OutputsPanel extends StatelessWidget {
  final List<OwnedOutput> allOutputs;
  final int currentHeight;
  final bool showSpentOutputs;
  final String sortBy;
  final bool sortAscending;
  final int activeAccount; // -1 means "All"
  final VoidCallback onToggleShowSpent;
  final VoidCallback onThawAll;
  final VoidCallback onFreezeAll;
  final Function(String sortKey) onSortChanged;
  final Function(String keyImage, bool freeze)? onFreezeChanged;
  final Set<String> pendingSpentKeyImages;

  const OutputsPanel({
    super.key,
    required this.allOutputs,
    required this.currentHeight,
    required this.showSpentOutputs,
    required this.sortBy,
    required this.sortAscending,
    required this.activeAccount,
    required this.onToggleShowSpent,
    required this.onThawAll,
    required this.onFreezeAll,
    required this.onSortChanged,
    this.onFreezeChanged,
    this.pendingSpentKeyImages = const {},
  });

  List<OwnedOutput> _sortedOutputs() {
    final outputs = showSpentOutputs
        ? List<OwnedOutput>.from(allOutputs)
        : allOutputs.where((o) => !o.spent).toList();

    final amountValues = <OwnedOutput, double>{};
    if (sortBy != 'confirms') {
      for (var o in outputs) {
        amountValues[o] = double.tryParse(o.amountXmr) ?? 0;
      }
    }

    outputs.sort((a, b) {
      int comparison;
      if (sortBy == 'confirms') {
        final aHeight = a.blockHeight;
        final bHeight = b.blockHeight;
        final aConfirms = aHeight > 0 ? currentHeight - aHeight : 0;
        final bConfirms = bHeight > 0 ? currentHeight - bHeight : 0;
        comparison = aConfirms.compareTo(bConfirms);
      } else {
        comparison = amountValues[a]!.compareTo(amountValues[b]!);
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

    final hasSpentOutputs = allOutputs.any((o) => o.spent);

    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Row(
              children: [
                if (hasSpentOutputs) ...[
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
                CommonWidgets.buildSelectButton(label: 'Thaw All', onPressed: onThawAll),
                const SizedBox(width: 4),
                CommonWidgets.buildSelectButton(label: 'Freeze All', onPressed: onFreezeAll),
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
          Builder(builder: (context) {
            final sortedOutputs = _sortedOutputs();
            return ListView.builder(
            shrinkWrap: true,
            physics: const NeverScrollableScrollPhysics(),
            itemCount: sortedOutputs.length,
            itemBuilder: (context, index) {
            final output = sortedOutputs[index];
            final outputHeight = output.blockHeight;
            final confirmations = outputHeight > 0
                ? currentHeight - outputHeight
                : 0;
            final isSpendable = OutputLockUtils.isOutputSpendable(output: output, currentHeight: currentHeight);
            final requiredConfirmations = OutputLockUtils.getRequiredConfirmations(output);
            final isFrozen = output.frozen;
            final isPendingSpend = pendingSpentKeyImages.contains(output.keyImage);
            final statusColor = output.spent
                ? Colors.grey
                : isPendingSpend
                    ? Colors.amber
                    : isSpendable
                        ? Colors.green
                        : Colors.orange;
            final statusText = output.spent
                ? 'SPENT'
                : isPendingSpend
                    ? 'PENDING SPEND'
                    : isSpendable
                        ? 'SPENDABLE'
                        : 'LOCKED ($confirmations/$requiredConfirmations)';

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
                        Text(
                          '${output.amountXmr} XMR',
                          style: TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 16,
                            color: output.spent ? Colors.grey.shade600 : Colors.black,
                          ),
                        ),
                        Row(
                          children: [
                            Container(
                              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                              decoration: BoxDecoration(
                                color: statusColor.withValues(alpha: 0.1),
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
                            if (!output.spent && !isPendingSpend) ...[
                              const SizedBox(width: 8),
                              SizedBox(
                                width: 24,
                                height: 24,
                                child: Checkbox(
                                  value: !isFrozen,
                                  onChanged: (value) {
                                    final freeze = !(value ?? true);
                                    onFreezeChanged?.call(output.keyImage, freeze);
                                  },
                                ),
                              ),
                              const SizedBox(width: 4),
                              Text(
                                isFrozen ? 'Frozen' : 'Spend',
                                style: TextStyle(
                                  fontSize: 10,
                                  fontWeight: FontWeight.bold,
                                  color: isFrozen ? Colors.blue.shade800 : Colors.green.shade800,
                                ),
                              ),
                            ],
                          ],
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    // Show account when "All" is selected
                    if (activeAccount == -1 && output.subaddressIndex != null)
                      CommonWidgets.buildOutputDetailRow(
                        label: 'Account',
                        value: '${output.subaddressIndex![0]}',
                      ),
                    if (activeAccount == -1 && output.subaddressIndex == null)
                      CommonWidgets.buildOutputDetailRow(
                        label: 'Account',
                        value: '0',
                      ),
                    CommonWidgets.buildOutputDetailRow(label: 'TX Hash', value: output.txHash, mono: true),
                    CommonWidgets.buildOutputDetailRow(label: 'Output Index', value: '${output.outputIndex}'),
                    CommonWidgets.buildOutputDetailRow(label: 'Block Height', value: '$outputHeight'),
                    if (output.subaddressIndex != null)
                      CommonWidgets.buildOutputDetailRow(
                        label: 'Subaddress',
                        value: '${output.subaddressIndex![0]}/${output.subaddressIndex![1]}',
                      ),
                    if (output.paymentId != null)
                      CommonWidgets.buildOutputDetailRow(label: 'Payment ID', value: output.paymentId!, mono: true),
                  ],
                ),
              ),
            );
          },
          );
          }),
        ],
      ),
    );
  }
}
