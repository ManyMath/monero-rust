import 'package:flutter/material.dart';
import '../models/wallet_transaction.dart';
import '../src/bindings/bindings.dart';
import 'common_widgets.dart';

/// Widget that displays transaction history with sorting and expandable details.
///
/// Shows transaction list with sorting controls, expandable transaction cards,
/// and balance change display for each transaction.
class TransactionsPanel extends StatelessWidget {
  final List<WalletTransaction> allTransactions;
  final List<OwnedOutput> allOutputs;
  final int currentHeight;
  final String txSortBy;
  final bool txSortAscending;
  final Set<String> expandedTransactions;
  final Function(String sortKey) onSortChanged;
  final Function(String txHash) onToggleExpanded;

  const TransactionsPanel({
    super.key,
    required this.allTransactions,
    required this.allOutputs,
    required this.currentHeight,
    required this.txSortBy,
    required this.txSortAscending,
    required this.expandedTransactions,
    required this.onSortChanged,
    required this.onToggleExpanded,
  });

  @override
  Widget build(BuildContext context) {
    if (allTransactions.isEmpty) {
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(16.0),
          child: Text(
            'No transactions found. Scan blocks to find transactions.',
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
          // Sort controls
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Row(
              children: [
                const Text('Sort: ', style: TextStyle(fontSize: 12)),
                CommonWidgets.buildTxSortButton(
                  label: 'Confirms',
                  sortKey: 'confirms',
                  currentSortKey: txSortBy,
                  isAscending: txSortAscending,
                  onTap: () => onSortChanged('confirms'),
                ),
                const SizedBox(width: 4),
                CommonWidgets.buildTxSortButton(
                  label: 'Amount',
                  sortKey: 'amount',
                  currentSortKey: txSortBy,
                  isAscending: txSortAscending,
                  onTap: () => onSortChanged('amount'),
                ),
              ],
            ),
          ),
          // Transaction cards
          ...allTransactions.map((tx) {
            final isExpanded = expandedTransactions.contains(tx.txHash);
            final balanceChange = tx.balanceChange(allOutputs);
            final isIncoming = balanceChange > 0;
            final confirmations = currentHeight > 0 && tx.blockHeight > 0
                ? currentHeight - tx.blockHeight
                : 0;
            final statusColor = isIncoming ? Colors.green : Colors.red;
            final amountStr = isIncoming
                ? '+${balanceChange.toStringAsFixed(12)}'
                : balanceChange.toStringAsFixed(12);
            final txIdDisplay = tx.txHash.startsWith('spend:')
                ? 'Outgoing (${tx.txHash.substring(6, 14)}...)'
                : '${tx.txHash.substring(0, 8)}...${tx.txHash.substring(tx.txHash.length - 8)}';

            return Card(
              margin: const EdgeInsets.only(bottom: 12),
              elevation: 2,
              child: InkWell(
                onTap: () => onToggleExpanded(tx.txHash),
                borderRadius: BorderRadius.circular(4),
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      // Summary row (always visible)
                      Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          Expanded(
                            child: Row(
                              children: [
                                Icon(
                                  isIncoming ? Icons.arrow_downward : Icons.arrow_upward,
                                  size: 16,
                                  color: statusColor,
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: Text(
                                    txIdDisplay,
                                    style: const TextStyle(
                                      fontFamily: 'monospace',
                                      fontSize: 12,
                                    ),
                                    overflow: TextOverflow.ellipsis,
                                  ),
                                ),
                              ],
                            ),
                          ),
                          const SizedBox(width: 8),
                          Text(
                            '$amountStr XMR',
                            style: TextStyle(
                              fontWeight: FontWeight.bold,
                              fontSize: 14,
                              color: statusColor,
                            ),
                          ),
                        ],
                      ),
                      // Confirmation count row
                      Padding(
                        padding: const EdgeInsets.only(top: 4),
                        child: Row(
                          children: [
                            Text(
                              '$confirmations confirmation${confirmations == 1 ? '' : 's'}',
                              style: TextStyle(
                                fontSize: 10,
                                color: Colors.grey.shade600,
                              ),
                            ),
                            const Spacer(),
                            Icon(
                              isExpanded ? Icons.expand_less : Icons.expand_more,
                              size: 16,
                              color: Colors.grey.shade600,
                            ),
                          ],
                        ),
                      ),
                      // Expanded details
                      if (isExpanded) ...[
                        const Divider(height: 16),
                        CommonWidgets.buildOutputDetailRow(
                          label: 'TX Hash',
                          value: tx.txHash.startsWith('spend:') ? 'Unknown (outgoing)' : tx.txHash,
                          mono: true,
                        ),
                        CommonWidgets.buildOutputDetailRow(label: 'Block Height', value: '${tx.blockHeight}'),
                        if (tx.blockTimestamp > 0)
                          CommonWidgets.buildOutputDetailRow(
                            label: 'Timestamp',
                            value: DateTime.fromMillisecondsSinceEpoch(tx.blockTimestamp * 1000).toString(),
                          ),
                        // Received outputs
                        if (tx.receivedOutputs.isNotEmpty) ...[
                          const SizedBox(height: 8),
                          Text(
                            'Received Outputs (${tx.receivedOutputs.length}):',
                            style: const TextStyle(
                              fontWeight: FontWeight.w500,
                              fontSize: 11,
                              color: Colors.black54,
                            ),
                          ),
                          const SizedBox(height: 4),
                          ...tx.receivedOutputs.map((output) {
                            final isSpent = output.spent;
                            return Container(
                              margin: const EdgeInsets.only(left: 8, bottom: 4),
                              padding: const EdgeInsets.all(8),
                              decoration: BoxDecoration(
                                color: Colors.green.shade50,
                                borderRadius: BorderRadius.circular(4),
                                border: Border.all(color: Colors.green.shade200),
                              ),
                              child: Row(
                                children: [
                                  Expanded(
                                    child: Text(
                                      '+${output.amountXmr} XMR (index ${output.outputIndex})',
                                      style: TextStyle(
                                        fontSize: 11,
                                        color: isSpent ? Colors.grey : Colors.green.shade800,
                                        decoration: isSpent ? TextDecoration.lineThrough : null,
                                      ),
                                    ),
                                  ),
                                  if (isSpent)
                                    Container(
                                      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                                      decoration: BoxDecoration(
                                        color: Colors.grey.shade200,
                                        borderRadius: BorderRadius.circular(2),
                                      ),
                                      child: const Text(
                                        'SPENT',
                                        style: TextStyle(fontSize: 8, color: Colors.grey),
                                      ),
                                    ),
                                ],
                              ),
                            );
                          }),
                        ],
                        // Spent outputs
                        if (tx.spentKeyImages.isNotEmpty) ...[
                          const SizedBox(height: 8),
                          Text(
                            'Spent Outputs (${tx.spentKeyImages.length}):',
                            style: const TextStyle(
                              fontWeight: FontWeight.w500,
                              fontSize: 11,
                              color: Colors.black54,
                            ),
                          ),
                          const SizedBox(height: 4),
                          ...tx.spentKeyImages.map((keyImage) {
                            final spentOutput = allOutputs.where((o) => o.keyImage == keyImage).firstOrNull;
                            final amountStr = spentOutput?.amountXmr ?? 'Unknown';
                            return Container(
                              margin: const EdgeInsets.only(left: 8, bottom: 4),
                              padding: const EdgeInsets.all(8),
                              decoration: BoxDecoration(
                                color: Colors.red.shade50,
                                borderRadius: BorderRadius.circular(4),
                                border: Border.all(color: Colors.red.shade200),
                              ),
                              child: Text(
                                '-$amountStr XMR',
                                style: TextStyle(
                                  fontSize: 11,
                                  color: Colors.red.shade800,
                                ),
                              ),
                            );
                          }),
                        ],
                      ],
                    ],
                  ),
                ),
              ),
            );
          }),
        ],
      ),
    );
  }
}
