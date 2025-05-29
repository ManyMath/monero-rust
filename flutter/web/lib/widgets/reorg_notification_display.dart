import 'package:flutter/material.dart';
import 'common_widgets.dart';

class ReorgNotificationDisplay extends StatelessWidget {
  final int splitHeight;
  final int blocksDetached;
  final int outputsRemoved;
  final int outputsUnspent;
  final VoidCallback onDismiss;

  const ReorgNotificationDisplay({
    super.key,
    required this.splitHeight,
    required this.blocksDetached,
    required this.outputsRemoved,
    required this.outputsUnspent,
    required this.onDismiss,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.orange.shade50,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: Colors.orange.shade200),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  'Chain Reorganization Detected',
                  style: TextStyle(
                    fontWeight: FontWeight.bold,
                    color: Colors.orange.shade900,
                    fontSize: 16,
                  ),
                ),
              ),
              IconButton(
                icon: Icon(Icons.close, color: Colors.orange.shade900, size: 18),
                onPressed: onDismiss,
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints(),
              ),
            ],
          ),
          const SizedBox(height: 8),
          CommonWidgets.buildScanResultRow(
            label: 'Split height',
            value: splitHeight.toString(),
          ),
          CommonWidgets.buildScanResultRow(
            label: 'Blocks detached',
            value: blocksDetached.toString(),
          ),
          CommonWidgets.buildScanResultRow(
            label: 'Outputs removed',
            value: outputsRemoved.toString(),
          ),
          CommonWidgets.buildScanResultRow(
            label: 'Outputs unspent',
            value: outputsUnspent.toString(),
          ),
          const SizedBox(height: 8),
          Text(
            'Wallet state has been rolled back and re-synced.',
            style: TextStyle(
              fontStyle: FontStyle.italic,
              color: Colors.orange.shade700,
              fontSize: 12,
            ),
          ),
        ],
      ),
    );
  }
}
