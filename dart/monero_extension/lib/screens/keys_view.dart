import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';

class KeysView extends StatefulWidget {
  const KeysView({super.key});

  @override
  State<KeysView> createState() => _KeysViewState();
}

class _KeysViewState extends State<KeysView> {
  final _controller = TextEditingController();
  String? _validationError;
  String? _derivedAddress;
  String? _responseError;
  bool _isLoading = false;

  @override
  void initState() {
    super.initState();

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
    _controller.dispose();
    super.dispose();
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
      network: 'stagenet',
    ).sendSignalToRust();
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
            TextField(
              controller: _controller,
              maxLines: 3,
              decoration: InputDecoration(
                labelText: '25-word seed',
                hintText: 'Enter your 25-word mnemonic seed',
                border: const OutlineInputBorder(),
                errorText: _validationError,
              ),
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                Expanded(
                  child: OutlinedButton(
                    onPressed: _generateSeed,
                    child: const Text('Generate'),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: ElevatedButton(
                    onPressed: _isLoading ? null : _deriveAddress,
                    child: _isLoading
                        ? const SizedBox(
                            height: 20,
                            width: 20,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Text('Derive Address'),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 24),
            if (_responseError != null)
              Container(
                padding: const EdgeInsets.all(12),
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
              Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: Colors.green.shade50,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: Colors.green.shade200),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Stagenet Address:',
                      style: TextStyle(
                        fontWeight: FontWeight.bold,
                        color: Colors.green.shade900,
                      ),
                    ),
                    const SizedBox(height: 8),
                    SelectableText(
                      _derivedAddress!,
                      style: const TextStyle(fontFamily: 'monospace'),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}
