import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../src/ffi/signal_types.dart';
import 'common_widgets.dart';

/// A dialog for generating and displaying payment proofs (OutProof or Tx Key).
///
/// Exposes a static [show] method that handles the full proof generation flow:
/// loading dialog, OutProof generation via Rust signal, and displaying the
/// formatted proof or falling back to a simple Tx Key dialog.
class PaymentProofDialog {
  static void show(
    BuildContext context, {
    required TransactionCreatedResponse? txResult,
    required List<TextEditingController> destinationControllers,
    required String network,
  }) {
    if (txResult == null) return;

    final txId = txResult.txId;
    final txKey = txResult.txKey ?? 'Not available';

    final recipients = <String>[];
    for (int i = 0; i < destinationControllers.length; i++) {
      final addr = destinationControllers[i].text.trim();
      if (addr.isNotEmpty) recipients.add(addr);
    }

    // Show loading dialog first
    showDialog(
      context: context,
      barrierDismissible: false,
      builder: (context) => const AlertDialog(
        content: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            CircularProgressIndicator(),
            SizedBox(width: 16),
            Text('Generating OutProof...'),
          ],
        ),
      ),
    );

    // Generate OutProof for the first recipient
    final recipientAddr = recipients.isNotEmpty ? recipients.first : '';
    if (recipientAddr.isEmpty || txKey == 'Not available') {
      Navigator.of(context).pop();
      _showSimpleProofDialog(context, txId, txKey, recipients);
      return;
    }

    // Subscribe to proof response
    StreamSubscription? sub;
    sub = OutProofGeneratedResponse.stream.listen((response) {
      sub?.cancel();
      Navigator.of(context).pop();

      if (response.success && response.formatted != null) {
        _showOutProofDialog(context, response.formatted!, txId, txKey, recipients);
      } else {
        _showSimpleProofDialog(context, txId, txKey, recipients);
      }
    });

    GenerateOutProofRequest(
      txId: txId,
      txKey: txKey,
      recipientAddress: recipientAddr,
      message: '',
      network: network,
    ).sendSignalToRust();
  }

  static void _showOutProofDialog(
    BuildContext context,
    String formattedProof,
    String txId,
    String txKey,
    List<String> recipients,
  ) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('OutProof'),
        content: SizedBox(
          width: 520,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: Colors.grey.shade100,
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: Colors.grey.shade300),
                  ),
                  child: SelectableText(
                    formattedProof,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 11),
                  ),
                ),
                if (recipients.length > 1) ...[
                  const SizedBox(height: 12),
                  Text(
                    'Note: Proof generated for first recipient only.',
                    style: TextStyle(fontSize: 11, color: Colors.grey.shade600),
                  ),
                ],
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () {
              Clipboard.setData(ClipboardData(text: formattedProof));
              ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('OutProof copied')));
            },
            child: const Text('Copy'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  static void _showSimpleProofDialog(
    BuildContext context,
    String txId,
    String txKey,
    List<String> recipients,
  ) {
    final allText = [
      'Tx ID: $txId',
      'Tx Key: $txKey',
      ...recipients.map((addr) => 'Address: $addr'),
    ].join('\n');

    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Tx Key'),
        content: SizedBox(
          width: 480,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              CommonWidgets.buildProofRow(label: 'Tx ID', value: txId, context: context),
              const SizedBox(height: 8),
              CommonWidgets.buildProofRow(label: 'Tx Key', value: txKey, context: context),
              if (recipients.isNotEmpty) ...[
                const SizedBox(height: 8),
                ...recipients.map((addr) => CommonWidgets.buildProofRow(label: 'Address', value: addr, context: context)),
              ],
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () {
              Clipboard.setData(ClipboardData(text: allText));
              ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Copied')));
            },
            child: const Text('Copy All'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }
}
