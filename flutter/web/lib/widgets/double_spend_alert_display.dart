import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import 'common_widgets.dart';

class DoubleSpendAlertDisplay extends StatelessWidget {
  final List<DoubleSpendConflict> conflicts;
  final VoidCallback onDismiss;

  const DoubleSpendAlertDisplay({
    super.key,
    required this.conflicts,
    required this.onDismiss,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.red.shade50,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: Colors.red.shade200),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.warning, color: Colors.red.shade900, size: 18),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  'Double-Spend Detected',
                  style: TextStyle(
                    fontWeight: FontWeight.bold,
                    color: Colors.red.shade900,
                    fontSize: 16,
                  ),
                ),
              ),
              IconButton(
                icon: Icon(Icons.close, color: Colors.red.shade900, size: 18),
                onPressed: onDismiss,
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints(),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text(
            'A key image was observed spent at conflicting heights. '
            'This may indicate a double-spend attempt against your wallet.',
            style: TextStyle(color: Colors.red.shade800, fontSize: 12),
          ),
          const SizedBox(height: 8),
          for (final c in conflicts) ...[
            CommonWidgets.buildScanResultRow(
              label: 'Key image',
              value: _truncate(c.keyImage.toString(), 20),
            ),
            CommonWidgets.buildScanResultRow(
              label: 'First seen',
              value: _heightLabel(c.previousSpentHeight),
            ),
            CommonWidgets.buildScanResultRow(
              label: 'Conflict at',
              value: _heightLabel(c.newHeight),
            ),
            if (c != conflicts.last) const SizedBox(height: 4),
          ],
        ],
      ),
    );
  }

  String _heightLabel(int height) {
    return height == 0 ? 'mempool' : height.toString();
  }

  String _truncate(String s, int maxLen) {
    if (s.length <= maxLen) return s;
    return '${s.substring(0, maxLen ~/ 2)}...${s.substring(s.length - maxLen ~/ 2)}';
  }
}
