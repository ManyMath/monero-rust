import 'package:flutter/material.dart';

class PasswordDialog extends StatefulWidget {
  final bool isUnlock;
  final String title;
  final String submitLabel;

  const PasswordDialog({
    super.key,
    required this.isUnlock,
    required this.title,
    required this.submitLabel,
  });

  @override
  State<PasswordDialog> createState() => _PasswordDialogState();
}

class _PasswordDialogState extends State<PasswordDialog> {
  final _passwordController = TextEditingController();
  final _confirmPasswordController = TextEditingController();
  String? _error;
  bool _obscurePassword = true;
  bool _obscureConfirm = true;

  @override
  void dispose() {
    _passwordController.dispose();
    _confirmPasswordController.dispose();
    super.dispose();
  }

  void _submit() {
    final password = _passwordController.text;

    if (!widget.isUnlock) {
      // Creating new password - require confirmation if not empty
      if (password.isNotEmpty) {
        final confirm = _confirmPasswordController.text;
        if (password != confirm) {
          setState(() {
            _error = 'Passwords do not match';
          });
          return;
        }
      }
    }

    Navigator.of(context).pop(password);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.title),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            controller: _passwordController,
            autofocus: true,
            obscureText: _obscurePassword,
            decoration: InputDecoration(
              labelText: widget.isUnlock ? 'Password' : 'Password (optional)',
              border: const OutlineInputBorder(),
              suffixIcon: IconButton(
                icon: Icon(_obscurePassword ? Icons.visibility : Icons.visibility_off),
                onPressed: () => setState(() => _obscurePassword = !_obscurePassword),
              ),
            ),
            onSubmitted: (_) {
              if (!widget.isUnlock || _passwordController.text.isEmpty) {
                _submit();
              }
            },
          ),
          // Only show confirm field when creating password and password is not empty
          if (!widget.isUnlock && _passwordController.text.isNotEmpty) ...[
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
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(null),
          child: const Text('Cancel'),
        ),
        ElevatedButton(
          onPressed: _submit,
          child: Text(widget.submitLabel),
        ),
      ],
    );
  }
}
