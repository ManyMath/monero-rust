import 'package:flutter/material.dart';

/// A confirmation dialog shown when importing a wallet with an existing ID.
///
/// Returns 'cancel', 'choose_different', or 'overwrite'.
class OverwriteWalletDialog {
  /// Shows a confirmation dialog when importing a wallet with an existing ID.
  /// Returns 'cancel', 'choose_different', or 'overwrite'.
  static Future<String?> show(BuildContext context, String walletId) {
    return showDialog<String>(
      context: context,
      barrierDismissible: false,
      builder: (context) => AlertDialog(
        title: const Text('Wallet Already Exists'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('A wallet with ID "$walletId" already exists.'),
            const SizedBox(height: 12),
            const Text(
              'What would you like to do?',
              style: TextStyle(fontWeight: FontWeight.bold),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop('cancel'),
            child: const Text('Cancel Import'),
          ),
          OutlinedButton(
            onPressed: () => Navigator.of(context).pop('choose_different'),
            child: const Text('Choose Different ID'),
          ),
          ElevatedButton(
            onPressed: () => Navigator.of(context).pop('overwrite'),
            style: ElevatedButton.styleFrom(
              backgroundColor: Colors.orange,
              foregroundColor: Colors.white,
            ),
            child: const Text('Overwrite Existing'),
          ),
        ],
      ),
    );
  }
}
