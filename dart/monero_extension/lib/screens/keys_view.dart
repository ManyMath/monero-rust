import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';

class KeysView extends StatefulWidget {
  const KeysView({super.key});

  @override
  State<KeysView> createState() => _KeysViewState();
}

class _KeysViewState extends State<KeysView> {
  final _controller = TextEditingController();
  String _network = 'stagenet';
  final String _seedType = '25 word';
  String? _validationError;
  String? _derivedAddress;
  String? _responseError;
  bool _isLoading = false;
  Timer? _debounceTimer;

  @override
  void initState() {
    super.initState();

    _controller.addListener(_onSeedChanged);

    AddressDerivedResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isLoading = false;
        if (signal.message.success) {
          _derivedAddress = signal.message.address;
          _responseError = null;
        } else {
          _derivedAddress = null;
          _responseError = signal.message.error ?? 'Unknown error';
        }
      });
    });

    SeedGeneratedResponse.rustSignalStream.listen((signal) {
      if (signal.message.success) {
        setState(() {
          _controller.text = signal.message.seed;
          _validationError = null;
          _responseError = null;
          _derivedAddress = null;
        });
      } else {
        setState(() {
          _responseError = signal.message.error ?? 'Failed to generate seed';
        });
      }
    });
  }

  @override
  void dispose() {
    _debounceTimer?.cancel();
    _controller.removeListener(_onSeedChanged);
    _controller.dispose();
    super.dispose();
  }

  void _onSeedChanged() {
    _debounceTimer?.cancel();

    _debounceTimer = Timer(const Duration(milliseconds: 800), () {
      _deriveAddress();
    });
  }

  void _generateSeed() {
    setState(() {
      _validationError = null;
      _responseError = null;
      _derivedAddress = null;
    });

    GenerateSeedRequest().sendSignalToRust();
  }

  void _deriveAddress() {
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _validationError = null;
        _responseError = null;
        _derivedAddress = null;
      });
      return;
    }

    setState(() {
      _validationError = null;
      _responseError = null;
      _derivedAddress = null;
    });

    final result = KeyParser.parse(_controller.text);

    if (!result.isValid) {
      setState(() {
        _validationError = result.error;
      });
      return;
    }

    setState(() {
      _isLoading = true;
    });

    DeriveAddressRequest(
      seed: result.normalizedInput!,
      network: _network,
    ).sendSignalToRust();
  }

  Future<void> _copyToClipboard(String text, String label) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('$label copied to clipboard'),
          duration: const Duration(seconds: 2),
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Keys View'),
      ),
      body: Padding(
        padding: const EdgeInsets.all(16.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: OutlinedButton.icon(
                    onPressed: _generateSeed,
                    icon: const Icon(Icons.refresh),
                    label: const Text('Generate'),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: DropdownButtonFormField<String>(
                    initialValue: _seedType,
                    decoration: const InputDecoration(
                      labelText: 'Seed Type',
                      border: OutlineInputBorder(),
                      contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    ),
                    items: const [
                      DropdownMenuItem(value: '25 word', child: Text('25 word')),
                    ],
                    onChanged: null,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: DropdownButtonFormField<String>(
                    initialValue: _network,
                    decoration: const InputDecoration(
                      labelText: 'Network',
                      border: OutlineInputBorder(),
                      contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    ),
                    items: const [
                      DropdownMenuItem(value: 'mainnet', child: Text('Mainnet')),
                      DropdownMenuItem(value: 'stagenet', child: Text('Stagenet')),
                      DropdownMenuItem(value: 'testnet', child: Text('Testnet')),
                    ],
                    onChanged: (value) {
                      if (value != null) {
                        setState(() {
                          _network = value;
                        });
                        _deriveAddress();
                      }
                    },
                  ),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: TextField(
                    controller: _controller,
                    maxLines: 3,
                    decoration: InputDecoration(
                      labelText: 'Seed',
                      hintText: 'Enter or generate a 25-word seed',
                      border: const OutlineInputBorder(),
                      errorText: _validationError,
                      suffixIcon: _isLoading
                          ? const Padding(
                              padding: EdgeInsets.all(12.0),
                              child: SizedBox(
                                width: 20,
                                height: 20,
                                child: CircularProgressIndicator(strokeWidth: 2),
                              ),
                            )
                          : null,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  onPressed: _controller.text.isNotEmpty
                      ? () => _copyToClipboard(_controller.text, 'Seed')
                      : null,
                  icon: const Icon(Icons.copy_outlined),
                  tooltip: 'Copy seed',
                ),
              ],
            ),
            const SizedBox(height: 24),
            if (_responseError != null)
              Container(
                padding: const EdgeInsets.all(12),
                margin: const EdgeInsets.only(bottom: 16),
                decoration: BoxDecoration(
                  color: Colors.red.shade50,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: Colors.red.shade200),
                ),
                child: Text(
                  'Error: $_responseError',
                  style: TextStyle(color: Colors.red.shade900),
                ),
              ),
            if (_derivedAddress != null)
              ExpansionTile(
                title: Text(
                  '${_network[0].toUpperCase()}${_network.substring(1)} Address',
                  style: const TextStyle(fontWeight: FontWeight.bold),
                ),
                subtitle: Row(
                  children: [
                    Expanded(
                      child: SelectableText(
                        _derivedAddress!,
                        style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                      ),
                    ),
                    IconButton(
                      icon: const Icon(Icons.copy_outlined, size: 16),
                      onPressed: () => _copyToClipboard(_derivedAddress!, 'Address'),
                      tooltip: 'Copy address',
                      padding: EdgeInsets.zero,
                      constraints: const BoxConstraints(),
                    ),
                  ],
                ),
                children: [
                  _buildKeyRow('Secret Spend Key', 'TODO'),
                  _buildKeyRow('Secret View Key', 'TODO'),
                  _buildKeyRow('Public Spend Key', 'TODO'),
                  _buildKeyRow('Public View Key', 'TODO'),
                ],
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildKeyRow(String label, String value) {
    final bool isTodo = value == 'TODO';
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                ),
                const SizedBox(height: 4),
                SelectableText(
                  value,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                ),
              ],
            ),
          ),
          IconButton(
            icon: const Icon(Icons.copy_outlined, size: 16),
            onPressed: !isTodo ? () => _copyToClipboard(value, label) : null,
            tooltip: isTodo ? null : 'Copy $label',
            padding: EdgeInsets.zero,
            constraints: const BoxConstraints(),
          ),
        ],
      ),
    );
  }
}
