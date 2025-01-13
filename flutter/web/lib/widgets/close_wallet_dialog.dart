import 'package:flutter/material.dart';

/// A confirmation dialog shown when closing a wallet.
///
/// Returns true to save & close, false to close without saving, null to cancel.
class CloseWalletDialog {
  /// Shows a dialog asking whether to save before closing a wallet.
  /// Returns true to save & close, false to close without saving, null to cancel.
  static Future<bool?> show(BuildContext context, String walletId) {
    return showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Close Wallet'),
        content: Text('Save changes to "$walletId" before closing?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Close Without Saving'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(null),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Save & Close'),
          ),
        ],
      ),
    );
  }
}
