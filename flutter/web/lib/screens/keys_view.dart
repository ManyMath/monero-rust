import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';
import '../services/extension_service.dart';

class KeysView extends StatefulWidget {
  const KeysView({super.key});

  @override
  State<KeysView> createState() => _KeysViewState();
}

class _KeysViewState extends State<KeysView> {
  final _controller = TextEditingController();
  final _extensionService = ExtensionService();
  final _nodeUrlController = TextEditingController(text: '127.0.0.1:38081');
  final _blockHeightController = TextEditingController();

  String _network = 'stagenet';
  final String _seedType = '25 word';
  String? _validationError;
  String? _derivedAddress;
  String? _responseError;
  bool _isLoading = false;
  bool _isScanning = false;
  Timer? _debounceTimer;
  String? _secretSpendKey;
  String? _secretViewKey;
  String? _publicSpendKey;
  String? _publicViewKey;

  BlockScanResponse? _scanResult;
  String? _scanError;

  int? _expandedPanel;

  @override
  void initState() {
    super.initState();

    _controller.addListener(_onSeedChanged);

    KeysDerivedResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isLoading = false;
        if (signal.message.success) {
          _derivedAddress = signal.message.address;
          _secretSpendKey = signal.message.secretSpendKey;
          _secretViewKey = signal.message.secretViewKey;
          _publicSpendKey = signal.message.publicSpendKey;
          _publicViewKey = signal.message.publicViewKey;
          _responseError = null;
        } else {
          _derivedAddress = null;
          _secretSpendKey = null;
          _secretViewKey = null;
          _publicSpendKey = null;
          _publicViewKey = null;
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

    BlockScanResponse.rustSignalStream.listen((signal) {
      setState(() {
        _isScanning = false;
        if (signal.message.success) {
          _scanResult = signal.message;
          _scanError = null;
        } else {
          _scanResult = null;
          _scanError = signal.message.error ?? 'Unknown error during scan';
        }
      });
    });
  }

  @override
  void dispose() {
    _debounceTimer?.cancel();
    _controller.removeListener(_onSeedChanged);
    _controller.dispose();
    _nodeUrlController.dispose();
    _blockHeightController.dispose();
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
      _secretSpendKey = null;
      _secretViewKey = null;
      _publicSpendKey = null;
      _publicViewKey = null;
    });

    GenerateSeedRequest().sendSignalToRust();
  }

  void _deriveAddress() {
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _validationError = null;
        _responseError = null;
        _derivedAddress = null;
        _secretSpendKey = null;
        _secretViewKey = null;
        _publicSpendKey = null;
        _publicViewKey = null;
      });
      return;
    }

    setState(() {
      _validationError = null;
      _responseError = null;
      _derivedAddress = null;
      _secretSpendKey = null;
      _secretViewKey = null;
      _publicSpendKey = null;
      _publicViewKey = null;
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

    DeriveKeysRequest(
      seed: result.normalizedInput!,
      network: _network,
    ).sendSignalToRust();
  }

  void _scanBlock() {
    if (_controller.text.trim().isEmpty) {
      setState(() {
        _scanError = 'Please enter a seed phrase first';
      });
      return;
    }

    final result = KeyParser.parse(_controller.text);
    if (!result.isValid) {
      setState(() {
        _scanError = 'Invalid seed phrase: ${result.error}';
      });
      return;
    }

    final blockHeightStr = _blockHeightController.text.trim();
    if (blockHeightStr.isEmpty) {
      setState(() {
        _scanError = 'Please enter a block height';
      });
      return;
    }

    final blockHeight = int.tryParse(blockHeightStr);
    if (blockHeight == null || blockHeight < 0) {
      setState(() {
        _scanError = 'Invalid block height';
      });
      return;
    }

    final nodeUrl = _nodeUrlController.text.trim();
    if (nodeUrl.isEmpty) {
      setState(() {
        _scanError = 'Please enter a node URL';
      });
      return;
    }

    setState(() {
      _isScanning = true;
      _scanResult = null;
      _scanError = null;
    });

    // Prepend http:// if not present
    final fullNodeUrl = nodeUrl.startsWith('http://') || nodeUrl.startsWith('https://')
        ? nodeUrl
        : 'http://$nodeUrl';

    ScanBlockRequest(
      nodeUrl: fullNodeUrl,
      blockHeight: Uint64(BigInt.from(blockHeight)),
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

  void _toggleViewMode() {
    _extensionService.isSidePanel
        ? _extensionService.openFullPage()
        : _extensionService.openSidePanel();
  }

  @override
  Widget build(BuildContext context) {
    final isSidePanel = _extensionService.isSidePanel;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Keys View'),
        actions: [
          if (_extensionService.isExtension)
            IconButton(
              icon: Icon(isSidePanel ? Icons.open_in_full : Icons.close_fullscreen),
              tooltip: isSidePanel ? 'Expand to Page' : 'Minimize to Side Panel',
              onPressed: _toggleViewMode,
            ),
        ],
      ),
      body: SingleChildScrollView(
        child: Padding(
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
                ExpansionPanelList(
                  expansionCallback: (int index, bool isExpanded) {
                    setState(() {
                      _expandedPanel = (_expandedPanel == index) ? null : index;
                    });
                  },
                  children: [
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return const ListTile(
                          title: Text(
                            'Keys',
                            style: TextStyle(fontWeight: FontWeight.bold),
                          ),
                        );
                      },
                      body: Column(
                        children: [
                          Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  '${_network[0].toUpperCase()}${_network.substring(1)} Address',
                                  style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                                ),
                                const SizedBox(height: 4),
                                Row(
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
                              ],
                            ),
                          ),
                          const Divider(height: 1),
                          _buildKeyRow('Secret Spend Key', _secretSpendKey ?? 'TODO'),
                          _buildKeyRow('Secret View Key', _secretViewKey ?? 'TODO'),
                          _buildKeyRow('Public Spend Key', _publicSpendKey ?? 'TODO'),
                          _buildKeyRow('Public View Key', _publicViewKey ?? 'TODO'),
                        ],
                      ),
                      isExpanded: _expandedPanel == 0,
                    ),
                    ExpansionPanel(
                      headerBuilder: (BuildContext context, bool isExpanded) {
                        return const ListTile(
                          title: Text(
                            'Scanning',
                            style: TextStyle(fontWeight: FontWeight.bold),
                          ),
                        );
                      },
                      body: Padding(
                        padding: const EdgeInsets.all(16.0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            TextField(
                              controller: _nodeUrlController,
                              decoration: const InputDecoration(
                                labelText: 'Node Address',
                                hintText: '127.0.0.1:38081',
                                border: OutlineInputBorder(),
                                helperText: 'For local stagenet node',
                              ),
                            ),
                            const SizedBox(height: 16),
                            TextField(
                              controller: _blockHeightController,
                              decoration: const InputDecoration(
                                labelText: 'Block Height',
                                hintText: 'Enter block height to scan',
                                border: OutlineInputBorder(),
                              ),
                              keyboardType: TextInputType.number,
                            ),
                            const SizedBox(height: 16),
                            ElevatedButton.icon(
                              onPressed: _isScanning ? null : _scanBlock,
                              icon: _isScanning
                                  ? const SizedBox(
                                      width: 16,
                                      height: 16,
                                      child: CircularProgressIndicator(strokeWidth: 2),
                                    )
                                  : const Icon(Icons.search),
                              label: Text(_isScanning ? 'Scanning...' : 'Scan Block'),
                            ),
                            if (_scanError != null) ...[
                              const SizedBox(height: 16),
                              Container(
                                padding: const EdgeInsets.all(12),
                                decoration: BoxDecoration(
                                  color: Colors.red.shade50,
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(color: Colors.red.shade200),
                                ),
                                child: Text(
                                  'Scan Error: $_scanError',
                                  style: TextStyle(color: Colors.red.shade900),
                                ),
                              ),
                            ],
                            if (_scanResult != null) ...[
                              const SizedBox(height: 16),
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
                                      'Scan Results',
                                      style: TextStyle(
                                        fontWeight: FontWeight.bold,
                                        color: Colors.green.shade900,
                                        fontSize: 16,
                                      ),
                                    ),
                                    const SizedBox(height: 8),
                                    _buildScanResultRow('Block Height', _scanResult!.blockHeight.toString()),
                                    _buildScanResultRow('Block Hash', _scanResult!.blockHash),
                                    _buildScanResultRow('Timestamp', DateTime.fromMillisecondsSinceEpoch(
                                      _scanResult!.blockTimestamp.toInt() * 1000,
                                    ).toString()),
                                    _buildScanResultRow('Transactions', _scanResult!.txCount.toString()),
                                    _buildScanResultRow('Outputs Found', _scanResult!.outputs.length.toString()),
                                    if (_scanResult!.outputs.isNotEmpty) ...[
                                      const Divider(height: 24),
                                      Text(
                                        'Owned Outputs:',
                                        style: TextStyle(
                                          fontWeight: FontWeight.bold,
                                          color: Colors.green.shade900,
                                        ),
                                      ),
                                      const SizedBox(height: 8),
                                      ..._scanResult!.outputs.map((output) => Card(
                                        margin: const EdgeInsets.only(bottom: 8),
                                        child: Padding(
                                          padding: const EdgeInsets.all(12),
                                          child: Column(
                                            crossAxisAlignment: CrossAxisAlignment.start,
                                            children: [
                                              Row(
                                                children: [
                                                  Text(
                                                    'Amount: ${output.amountXmr} XMR',
                                                    style: const TextStyle(
                                                      fontWeight: FontWeight.bold,
                                                      fontSize: 14,
                                                    ),
                                                  ),
                                                ],
                                              ),
                                              const SizedBox(height: 4),
                                              Text('TX Hash: ${output.txHash}', style: const TextStyle(fontSize: 10, fontFamily: 'monospace')),
                                              Text('Output Index: ${output.outputIndex}', style: const TextStyle(fontSize: 10)),
                                              if (output.subaddressIndex != null)
                                                Text('Subaddress: ${output.subaddressIndex!.item1}/${output.subaddressIndex!.item2}', style: const TextStyle(fontSize: 10)),
                                              if (output.paymentId != null)
                                                Text('Payment ID: ${output.paymentId}', style: const TextStyle(fontSize: 10)),
                                            ],
                                          ),
                                        ),
                                      )),
                                    ],
                                  ],
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                      isExpanded: _expandedPanel == 1,
                    ),
                  ],
                ),
            ],
          ),
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

  Widget _buildScanResultRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 120,
            child: Text(
              '$label:',
              style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 12),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: const TextStyle(fontSize: 12, fontFamily: 'monospace'),
            ),
          ),
        ],
      ),
    );
  }
}
