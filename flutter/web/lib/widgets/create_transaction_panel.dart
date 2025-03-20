import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../utils/output_utils.dart';
import 'recipient_form.dart';
import 'transaction_success_display.dart';
import 'broadcast_success_display.dart';
import 'error_message_container.dart';

class CreateTransactionPanel extends StatelessWidget {
  final List<TextEditingController> destinationControllers;
  final List<TextEditingController> amountControllers;
  final bool isCreatingTx;
  final bool isBroadcasting;
  final TransactionCreatedResponse? txResult;
  final TransactionBroadcastResponse? broadcastResult;
  final String? txError;
  final String? broadcastError;
  final String? multiAccountWarning;
  final VoidCallback onAddRecipient;
  final Function(int) onRemoveRecipient;
  final VoidCallback onCreateTransaction;
  final VoidCallback onBroadcastTransaction;
  final VoidCallback? onProvePayment;
  final VoidCallback onAmountChanged;
  final Function(int)? onSendMax;

  const CreateTransactionPanel({
    super.key,
    required this.destinationControllers,
    required this.amountControllers,
    required this.isCreatingTx,
    required this.isBroadcasting,
    required this.txResult,
    required this.broadcastResult,
    required this.txError,
    required this.broadcastError,
    this.multiAccountWarning,
    required this.onAddRecipient,
    required this.onRemoveRecipient,
    required this.onCreateTransaction,
    required this.onBroadcastTransaction,
    this.onProvePayment,
    required this.onAmountChanged,
    this.onSendMax,
  });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Recipients: ${destinationControllers.length}/15',
                style: const TextStyle(fontWeight: FontWeight.bold),
              ),
              Text(
                'Total: ${OutputUtils.getRecipientsTotal(amountControllers).toStringAsFixed(12)} XMR',
                style: const TextStyle(fontWeight: FontWeight.bold, color: Colors.blue),
              ),
            ],
          ),
          const SizedBox(height: 12),
          ...List.generate(destinationControllers.length, (index) {
            return RecipientForm(
              index: index,
              destinationController: destinationControllers[index],
              amountController: amountControllers[index],
              canRemove: destinationControllers.length > 1,
              onRemove: () => onRemoveRecipient(index),
              onAmountChanged: onAmountChanged,
              onSendMax: onSendMax != null ? () => onSendMax!(index) : null,
            );
          }),
          if (destinationControllers.length < 15)
            OutlinedButton.icon(
              onPressed: onAddRecipient,
              icon: const Icon(Icons.add),
              label: const Text('Add Recipient'),
            ),
          const SizedBox(height: 16),
          if (multiAccountWarning != null) ...[
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.orange.shade50,
                border: Border.all(color: Colors.orange.shade300),
                borderRadius: BorderRadius.circular(4),
              ),
              child: Row(
                children: [
                  Icon(Icons.warning_amber, color: Colors.orange.shade700, size: 20),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      multiAccountWarning!,
                      style: TextStyle(
                        fontSize: 12,
                        color: Colors.orange.shade900,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 12),
          ],
          ElevatedButton.icon(
            onPressed: isCreatingTx ? null : onCreateTransaction,
            icon: isCreatingTx
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.send),
            label: Text(isCreatingTx ? 'Sending...' : 'Send'),
          ),
          if (txError != null) ...[
            const SizedBox(height: 16),
            ErrorMessageContainer(message: 'Transaction Error: $txError'),
          ],
          if (txResult != null && txResult!.success) ...[
            const SizedBox(height: 16),
            TransactionSuccessDisplay(
              txResult: txResult!,
              isBroadcasting: isBroadcasting,
              onBroadcast: onBroadcastTransaction,
            ),
          ],
          if (broadcastError != null) ...[
            const SizedBox(height: 16),
            ErrorMessageContainer(message: 'Broadcast Error: $broadcastError'),
          ],
          if (broadcastResult != null && broadcastResult!.success) ...[
            const SizedBox(height: 16),
            BroadcastSuccessDisplay(
              hasTxKey: txResult?.txKey != null,
              onProvePayment: onProvePayment,
            ),
          ],
        ],
      ),
    );
  }
}
