import 'package:flutter/material.dart';

/// A dialog for saving wallet data with wallet ID and password inputs.
///
/// Returns a Map<String, String> with 'walletId' and 'password' keys when saved,
/// or null if cancelled.
class SaveWalletDialog extends StatefulWidget {
  final String initialWalletId;
  final List<String> existingWalletIds;

  const SaveWalletDialog({
    super.key,
    required this.initialWalletId,
    required this.existingWalletIds,
  });

  @override
  State<SaveWalletDialog> createState() => _SaveWalletDialogState();
}

class _SaveWalletDialogState extends State<SaveWalletDialog> {
  late final TextEditingController _walletIdController;
  final _passwordController = TextEditingController();
  final _confirmPasswordController = TextEditingController();
  String? _error;
  bool _obscurePassword = true;
  bool _obscureConfirm = true;

  @override
  void initState() {
    super.initState();
    _walletIdController = TextEditingController(text: widget.initialWalletId);
  }

  @override
  void dispose() {
    _walletIdController.dispose();
    _passwordController.dispose();
    _confirmPasswordController.dispose();
    super.dispose();
  }

  void _submit() {
    final walletId = _walletIdController.text.trim();
    final password = _passwordController.text;

    // Validate wallet ID
    if (walletId.isEmpty) {
      setState(() {
        _error = 'Wallet ID is required';
      });
      return;
    }

    // Check for invalid characters
    if (!RegExp(r'^[a-zA-Z0-9_-]+$').hasMatch(walletId)) {
      setState(() {
        _error = 'Wallet ID can only contain letters, numbers, underscores, and dashes';
      });
      return;
    }

    // Validate password confirmation if password is not empty
    if (password.isNotEmpty) {
      final confirm = _confirmPasswordController.text;
      if (password != confirm) {
        setState(() {
          _error = 'Passwords do not match';
        });
        return;
      }
    }

    Navigator.of(context).pop({
      'walletId': walletId,
      'password': password,
    });
  }

  @override
  Widget build(BuildContext context) {
    final isOverwriting = widget.existingWalletIds.contains(_walletIdController.text.trim());

    return AlertDialog(
      title: const Text('Save Wallet Data'),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _walletIdController,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Wallet ID',
                hintText: 'e.g., my_wallet, savings',
                border: OutlineInputBorder(),
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) => _submit(),
            ),
            if (isOverwriting) ...[
              const SizedBox(height: 8),
              Container(
                padding: const EdgeInsets.all(8),
                decoration: BoxDecoration(
                  color: Colors.orange.shade50,
                  borderRadius: BorderRadius.circular(4),
                  border: Border.all(color: Colors.orange.shade200),
                ),
                child: Row(
                  children: [
                    Icon(Icons.warning, color: Colors.orange.shade700, size: 16),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        'This will overwrite existing wallet data',
                        style: TextStyle(color: Colors.orange.shade900, fontSize: 12),
                      ),
                    ),
                  ],
                ),
              ),
            ],
            const SizedBox(height: 16),
            TextField(
              controller: _passwordController,
              obscureText: _obscurePassword,
              decoration: InputDecoration(
                labelText: 'Password (optional)',
                border: const OutlineInputBorder(),
                suffixIcon: IconButton(
                  icon: Icon(_obscurePassword ? Icons.visibility : Icons.visibility_off),
                  onPressed: () => setState(() => _obscurePassword = !_obscurePassword),
                ),
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) {
                if (_passwordController.text.isEmpty) {
                  _submit();
                }
              },
            ),
            if (_passwordController.text.isNotEmpty) ...[
              const SizedBox(height: 16),
              TextField(
                controller: _confirmPasswordController,
                obscureText: _obscureConfirm,
                decoration: InputDecoration(
                  labelText: 'Confirm Password',
                  border: const OutlineInputBorder(),
                  suffixIcon: IconButton(
                    icon: Icon(_obscureConfirm ? Icons.visibility : Icons.visibility_off),
                    onPressed: () => setState(() => _obscureConfirm = !_obscureConfirm),
                  ),
                ),
                onSubmitted: (_) => _submit(),
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
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(null),
          child: const Text('Cancel'),
        ),
        ElevatedButton(
          onPressed: _submit,
          child: const Text('Save'),
        ),
      ],
    );
  }
}
