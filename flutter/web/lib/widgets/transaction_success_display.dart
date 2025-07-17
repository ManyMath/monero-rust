import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../widgets/common_widgets.dart';

class TransactionSuccessDisplay extends StatelessWidget {
  final TransactionCreatedResponse txResult;
  final bool isBroadcasting;
  final VoidCallback onBroadcast;

  const TransactionSuccessDisplay({
    super.key,
    required this.txResult,
    required this.isBroadcasting,
    required this.onBroadcast,
  });

  static const int _feeAnomalyThreshold = 10000000000; // 0.01 XMR

  bool get _isFeeAnomaly {
    return txResult.fee > _feeAnomalyThreshold;
  }

  @override
  Widget build(BuildContext context) {
    final feeXmr = (txResult.fee / 1e12).toStringAsFixed(12);
    final feeAnomaly = _isFeeAnomaly;

    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.green.shade50,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: Colors.green.shade200),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Transaction Created',
            style: TextStyle(
              fontWeight: FontWeight.bold,
              color: Colors.green.shade900,
              fontSize: 16,
            ),
          ),
          const SizedBox(height: 8),
          CommonWidgets.buildScanResultRow(label: 'TX ID', value: txResult.txId),
          CommonWidgets.buildScanResultRow(
            label: 'Fee',
            value: '$feeXmr XMR',
          ),
          if (feeAnomaly) ...[
            const SizedBox(height: 8),
            Container(
              padding: const EdgeInsets.all(8),
              decoration: BoxDecoration(
                color: Colors.red.shade50,
                borderRadius: BorderRadius.circular(4),
                border: Border.all(color: Colors.red.shade400),
              ),
              child: Row(
                children: [
                  Icon(Icons.warning, color: Colors.red.shade700, size: 20),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      'Warning: Unusually high fee (> 0.01 XMR). '
                      'This may indicate a malicious node returning inflated fee rates.',
                      style: TextStyle(
                        color: Colors.red.shade900,
                        fontSize: 12,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
          if (txResult.txBlob != null)
            CommonWidgets.buildScanResultRow(
              label: 'TX Blob',
              value: '${txResult.txBlob!.substring(0, 64)}...',
            ),
          const SizedBox(height: 12),
          ElevatedButton.icon(
            onPressed: isBroadcasting ? null : onBroadcast,
            icon: isBroadcasting
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.upload),
            label: Text(isBroadcasting ? 'Broadcasting...' : 'Broadcast Transaction'),
            style: ElevatedButton.styleFrom(
              backgroundColor: Colors.green.shade700,
              foregroundColor: Colors.white,
            ),
          ),
        ],
      ),
    );
  }
}
