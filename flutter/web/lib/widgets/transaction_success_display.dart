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

  @override
  Widget build(BuildContext context) {
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
            value: '${(txResult.fee.toInt() / 1e12).toStringAsFixed(12)} XMR',
          ),
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
