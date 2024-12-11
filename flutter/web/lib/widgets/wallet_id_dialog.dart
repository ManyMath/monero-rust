import 'package:flutter/material.dart';

class WalletIdDialog extends StatefulWidget {
  final String suggestedWalletId;
  final List<String> existingWalletIds;

  const WalletIdDialog({
    super.key,
    required this.suggestedWalletId,
    required this.existingWalletIds,
  });

  @override
  State<WalletIdDialog> createState() => _WalletIdDialogState();
}

class _WalletIdDialogState extends State<WalletIdDialog> {
  late final TextEditingController _controller;
  String? _error;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.suggestedWalletId);
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final walletId = _controller.text.trim();

    if (walletId.isEmpty) {
      setState(() {
        _error = 'Wallet ID cannot be empty';
      });
      return;
    }

    // Check for invalid characters
    final validIdRegex = RegExp(r'^[a-zA-Z0-9_-]+$');
    if (!validIdRegex.hasMatch(walletId)) {
      setState(() {
        _error =
            'Wallet ID can only contain letters, numbers, hyphens, and underscores';
      });
      return;
    }

    // Allow selecting existing wallet ID - the import flow will handle overwrite confirmation
    Navigator.of(context).pop(walletId);
  }

  bool get _walletExists => widget.existingWalletIds.contains(_controller.text.trim());

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Import Wallet'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text(
            'Choose a name for this wallet:',
            style: TextStyle(fontSize: 14),
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _controller,
            autofocus: true,
            decoration: const InputDecoration(
              labelText: 'Wallet ID',
              border: OutlineInputBorder(),
              helperText: 'Letters, numbers, hyphens, and underscores only',
            ),
            onSubmitted: (_) => _submit(),
            onChanged: (_) => setState(() {}), // Rebuild to update warning
          ),
          if (_walletExists) ...[
            const SizedBox(height: 12),
            Container(
              padding: const EdgeInsets.all(8),
              decoration: BoxDecoration(
                color: Colors.orange.shade50,
                borderRadius: BorderRadius.circular(4),
                border: Border.all(color: Colors.orange.shade300),
              ),
              child: Row(
                children: [
                  Icon(Icons.warning_amber, color: Colors.orange.shade700, size: 16),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      'A wallet with this ID already exists. You will be asked to confirm overwrite.',
                      style: TextStyle(color: Colors.orange.shade900, fontSize: 12),
                    ),
                  ),
                ],
              ),
            ),
          ],
          if (_error != null) ...[
            const SizedBox(height: 12),
            Text(
              _error!,
              style: TextStyle(color: Colors.red.shade700, fontSize: 12),
            ),
          ],
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(null),
          child: const Text('Cancel'),
        ),
        ElevatedButton(
          onPressed: _submit,
          child: const Text('Continue'),
        ),
      ],
    );
  }
}
