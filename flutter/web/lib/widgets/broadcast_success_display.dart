import 'package:flutter/material.dart';

class BroadcastSuccessDisplay extends StatelessWidget {
  final bool hasTxKey;
  final VoidCallback? onProvePayment;

  const BroadcastSuccessDisplay({
    super.key,
    required this.hasTxKey,
    this.onProvePayment,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.blue.shade50,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: Colors.blue.shade200),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Transaction Broadcast Successfully!',
            style: TextStyle(
              fontWeight: FontWeight.bold,
              color: Colors.blue.shade900,
              fontSize: 16,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            'The transaction has been submitted to the network.',
            style: TextStyle(color: Colors.blue.shade900),
          ),
          if (hasTxKey && onProvePayment != null) ...[
            const SizedBox(height: 12),
            ElevatedButton(
              onPressed: onProvePayment,
              style: ElevatedButton.styleFrom(
                backgroundColor: Colors.blue.shade700,
                foregroundColor: Colors.white,
              ),
              child: const Text('Prove Payment'),
            ),
          ],
        ],
      ),
    );
  }
}
