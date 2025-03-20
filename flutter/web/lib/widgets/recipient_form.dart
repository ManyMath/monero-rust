import 'package:flutter/material.dart';

class RecipientForm extends StatelessWidget {
  final int index;
  final TextEditingController destinationController;
  final TextEditingController amountController;
  final bool canRemove;
  final VoidCallback onRemove;
  final VoidCallback onAmountChanged;
  final VoidCallback? onSendMax;

  const RecipientForm({
    super.key,
    required this.index,
    required this.destinationController,
    required this.amountController,
    required this.canRemove,
    required this.onRemove,
    required this.onAmountChanged,
    this.onSendMax,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: 12),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        border: Border.all(color: Colors.grey.shade300),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Text(
                'Recipient ${index + 1}',
                style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 12),
              ),
              const Spacer(),
              if (canRemove)
                IconButton(
                  icon: const Icon(Icons.close, size: 18),
                  onPressed: onRemove,
                  tooltip: 'Remove recipient',
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(),
                ),
            ],
          ),
          const SizedBox(height: 8),
          TextField(
            controller: destinationController,
            decoration: const InputDecoration(
              labelText: 'Address',
              hintText: 'Enter recipient Monero address',
              border: OutlineInputBorder(),
              contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 12),
            ),
            style: const TextStyle(fontSize: 12),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: amountController,
                  decoration: const InputDecoration(
                    labelText: 'Amount (XMR)',
                    border: OutlineInputBorder(),
                    contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                  ),
                  keyboardType: const TextInputType.numberWithOptions(decimal: true),
                  style: const TextStyle(fontSize: 12),
                  onChanged: (_) => onAmountChanged(),
                ),
              ),
              if (onSendMax != null) ...[
                const SizedBox(width: 8),
                OutlinedButton(
                  onPressed: onSendMax,
                  style: OutlinedButton.styleFrom(
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                  ),
                  child: const Text('Max', style: TextStyle(fontSize: 12)),
                ),
              ],
            ],
          ),
        ],
      ),
    );
  }
}
