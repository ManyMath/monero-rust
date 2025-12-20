import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import '../src/ffi/signal_types.dart';
import 'animated_qr_display.dart';
import 'dart:html' as html;

enum OfflineSignStep {
  showUnsignedQr,
  importSignedTx,
  importUnsignedTx,
  signing,
  showSignedQr,
  done,
}

/// Dialog for air-gapped offline signing via QR codes or file transfer.
///
/// View-only wallet flow: display unsigned TX as QR -> import signed TX -> broadcast
/// Full wallet flow: import unsigned TX -> sign -> display signed TX as QR / download
class OfflineSigningDialog extends StatefulWidget {
  final String? unsignedTxHex;
  final int fee;
  final bool isViewOnly;
  final String? seed;
  final String? network;

  const OfflineSigningDialog({
    super.key,
    this.unsignedTxHex,
    this.fee = 0,
    required this.isViewOnly,
    this.seed,
    this.network,
  });

  static Future<TransactionSignedOfflineResponse?> show(
    BuildContext context, {
    String? unsignedTxHex,
    int fee = 0,
    required bool isViewOnly,
    String? seed,
    String? network,
  }) {
    return showDialog<TransactionSignedOfflineResponse?>(
      context: context,
      barrierDismissible: false,
      builder: (_) => OfflineSigningDialog(
        unsignedTxHex: unsignedTxHex,
        fee: fee,
        isViewOnly: isViewOnly,
        seed: seed,
        network: network,
      ),
    );
  }

  @override
  State<OfflineSigningDialog> createState() => _OfflineSigningDialogState();
}

class _OfflineSigningDialogState extends State<OfflineSigningDialog> {
  late OfflineSignStep _step;
  String? _unsignedTxHex;
  String? _signedTxBlob;
  String? _signedTxId;
  int _fee = 0;
  String? _error;
  final _importController = TextEditingController();

  StreamSubscription? _signSub;

  @override
  void initState() {
    super.initState();
    _unsignedTxHex = widget.unsignedTxHex;
    _fee = widget.fee;
    if (widget.isViewOnly && _unsignedTxHex != null) {
      _step = OfflineSignStep.showUnsignedQr;
    } else if (!widget.isViewOnly) {
      _step = OfflineSignStep.importUnsignedTx;
    } else {
      _step = OfflineSignStep.importSignedTx;
    }
  }

  @override
  void dispose() {
    _signSub?.cancel();
    _importController.dispose();
    super.dispose();
  }

  void _downloadFile(String hex, String filename) {
    final bytes = _hexDecode(hex);
    final blob = html.Blob([bytes], 'application/octet-stream');
    final url = html.Url.createObjectUrlFromBlob(blob);
    html.AnchorElement(href: url)
      ..setAttribute('download', filename)
      ..click();
    html.Url.revokeObjectUrl(url);
  }

  Future<String?> _pickFile() async {
    final upload = html.FileUploadInputElement();
    upload.accept = '.bin,*';
    upload.click();
    try {
      await upload.onChange.first.timeout(const Duration(seconds: 120));
    } on TimeoutException {
      return null;
    }
    final files = upload.files;
    if (files == null || files.isEmpty) return null;
    final reader = html.FileReader();
    reader.readAsDataUrl(files[0]);
    await reader.onLoad.first;
    final dataUrl = reader.result as String;
    final bytes = base64Decode(dataUrl.substring(dataUrl.indexOf(',') + 1));
    return _hexEncode(Uint8List.fromList(bytes));
  }

  void _signOffline(String unsignedHex) {
    if (widget.seed == null || widget.network == null) {
      setState(() => _error = 'No seed available for signing');
      return;
    }
    setState(() {
      _step = OfflineSignStep.signing;
      _error = null;
    });

    _signSub = TransactionSignedOfflineResponse.stream.listen((msg) {
      _signSub?.cancel();
      _signSub = null;
      if (!mounted) return;
      if (msg.success) {
        setState(() {
          _signedTxBlob = msg.txBlob;
          _signedTxId = msg.txId;
          _fee = msg.fee.toInt();
          _step = OfflineSignStep.showSignedQr;
        });
      } else {
        setState(() {
          _error = msg.error ?? 'Signing failed';
          _step = OfflineSignStep.importUnsignedTx;
        });
      }
    });

    SignUnsignedTransactionRequest(
      seed: widget.seed!,
      unsignedTxHex: unsignedHex,
      network: widget.network!,
      passphrase: '',
      bip39AccountIndex: 0,
    ).sendSignalToRust();
  }

  void _importSignedTxFromText() {
    final hex = _importController.text.trim();
    if (hex.isEmpty) {
      setState(() => _error = 'Paste signed transaction hex');
      return;
    }
    Navigator.of(context).pop(
      TransactionSignedOfflineResponse(
        success: true,
        txBlob: hex,
        fee: _fee,
        txKeyAdditional: const [],
        changeOutputs: const [],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(_titleForStep()),
      content: SizedBox(
        width: 400,
        child: SingleChildScrollView(child: _buildStepContent()),
      ),
      actions: _buildActions(),
    );
  }

  String _titleForStep() {
    switch (_step) {
      case OfflineSignStep.showUnsignedQr:
        return 'Unsigned Transaction';
      case OfflineSignStep.importSignedTx:
        return 'Import Signed Transaction';
      case OfflineSignStep.importUnsignedTx:
        return 'Import Unsigned Transaction';
      case OfflineSignStep.signing:
        return 'Signing...';
      case OfflineSignStep.showSignedQr:
        return 'Signed Transaction';
      case OfflineSignStep.done:
        return 'Done';
    }
  }

  Widget _buildStepContent() {
    switch (_step) {
      case OfflineSignStep.showUnsignedQr:
        return _buildShowUnsignedQr();
      case OfflineSignStep.importSignedTx:
        return _buildImportTx(signed: true);
      case OfflineSignStep.importUnsignedTx:
        return _buildImportTx(signed: false);
      case OfflineSignStep.signing:
        return const Center(
          child: Padding(
            padding: EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                CircularProgressIndicator(),
                SizedBox(height: 16),
                Text('Signing transaction offline...'),
              ],
            ),
          ),
        );
      case OfflineSignStep.showSignedQr:
        return _buildShowSignedQr();
      case OfflineSignStep.done:
        return const Text('Transaction ready for broadcast.');
    }
  }

  Widget _buildShowUnsignedQr() {
    final feeXmr = (_fee / 1e12).toStringAsFixed(12);
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          'Fee: $feeXmr XMR',
          style: const TextStyle(fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 8),
        const Text('Scan this QR code with your offline signing device:'),
        const SizedBox(height: 12),
        Center(
          child: AnimatedQrDisplay(
            dataHex: _unsignedTxHex!,
            urType: 'xmr-txunsigned',
          ),
        ),
        const SizedBox(height: 12),
        OutlinedButton.icon(
          onPressed: () => _downloadFile(_unsignedTxHex!, 'unsigned_tx.bin'),
          icon: const Icon(Icons.download, size: 16),
          label: const Text('Download File'),
        ),
        if (_error != null) ...[
          const SizedBox(height: 8),
          Text(_error!, style: const TextStyle(color: Colors.red)),
        ],
      ],
    );
  }

  Widget _buildImportTx({required bool signed}) {
    final label = signed ? 'signed' : 'unsigned';
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text('Import $label transaction data:'),
        const SizedBox(height: 12),
        TextField(
          controller: _importController,
          decoration: InputDecoration(
            labelText: '${signed ? 'Signed' : 'Unsigned'} TX hex',
            border: const OutlineInputBorder(),
          ),
          maxLines: 3,
        ),
        const SizedBox(height: 12),
        Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: () async {
                  final hex = await _pickFile();
                  if (hex != null && mounted) {
                    if (signed) {
                      Navigator.of(context).pop(
                        TransactionSignedOfflineResponse(
                          success: true,
                          txBlob: hex,
                          fee: _fee,
                          txKeyAdditional: const [],
                          changeOutputs: const [],
                        ),
                      );
                    } else {
                      _signOffline(hex);
                    }
                  }
                },
                icon: const Icon(Icons.upload_file, size: 16),
                label: const Text('From File'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: ElevatedButton.icon(
                onPressed: () {
                  final hex = _importController.text.trim();
                  if (hex.isEmpty) {
                    setState(() => _error = 'Paste $label TX hex');
                    return;
                  }
                  if (signed) {
                    _importSignedTxFromText();
                  } else {
                    _signOffline(hex);
                  }
                },
                icon: const Icon(Icons.check, size: 16),
                label: const Text('From Text'),
              ),
            ),
          ],
        ),
        if (_error != null) ...[
          const SizedBox(height: 8),
          Text(_error!, style: const TextStyle(color: Colors.red)),
        ],
      ],
    );
  }

  Widget _buildShowSignedQr() {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (_signedTxId != null)
          SelectableText(
            'TX ID: $_signedTxId',
            style: const TextStyle(fontSize: 11),
          ),
        const SizedBox(height: 8),
        const Text('Scan this QR code with your online wallet to broadcast:'),
        const SizedBox(height: 12),
        if (_signedTxBlob != null)
          Center(
            child: AnimatedQrDisplay(
              dataHex: _signedTxBlob!,
              urType: 'xmr-txsigned',
            ),
          ),
        const SizedBox(height: 12),
        if (_signedTxBlob != null)
          OutlinedButton.icon(
            onPressed: () => _downloadFile(_signedTxBlob!, 'signed_tx.bin'),
            icon: const Icon(Icons.download, size: 16),
            label: const Text('Download File'),
          ),
      ],
    );
  }

  List<Widget> _buildActions() {
    return [
      TextButton(
        onPressed: () => Navigator.of(context).pop(null),
        child: const Text('Cancel'),
      ),
      if (_step == OfflineSignStep.showUnsignedQr)
        ElevatedButton(
          onPressed: () =>
              setState(() => _step = OfflineSignStep.importSignedTx),
          child: const Text('Import Signed TX'),
        ),
      if (_step == OfflineSignStep.showSignedQr && _signedTxBlob != null)
        ElevatedButton(
          onPressed: () {
            Navigator.of(context).pop(
              TransactionSignedOfflineResponse(
                success: true,
                txId: _signedTxId,
                txBlob: _signedTxBlob,
                fee: _fee,
                txKeyAdditional: const [],
                changeOutputs: const [],
              ),
            );
          },
          child: const Text('Done'),
        ),
    ];
  }
}

String _hexEncode(Uint8List bytes) {
  return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
}

Uint8List _hexDecode(String hex) {
  final result = Uint8List(hex.length ~/ 2);
  for (int i = 0; i < result.length; i++) {
    result[i] = int.parse(hex.substring(i * 2, i * 2 + 2), radix: 16);
  }
  return result;
}
