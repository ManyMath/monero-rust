import 'package:flutter/material.dart';

class ErrorMessageContainer extends StatelessWidget {
  final String message;

  const ErrorMessageContainer({
    super.key,
    required this.message,
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
      child: SelectableText(
        message,
        style: TextStyle(color: Colors.red.shade900),
      ),
    );
  }
}
