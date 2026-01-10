import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import '../src/ffi/signal_types.dart';
import '../utils/offline_signing_import_utils.dart';
import 'animated_qr_display.dart';
import 'dart:html' as html;

enum OfflineSignStep {
  showUnsignedQr,
  importSignedTx,
  importUnsignedTx,
  signing,
  buildingSignedTxSet,
  extractingSignedTxSet,
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
  final String? viewKeyHex;
  final List<String> wallet2KeyImages;
  final List<SignedTxSetKeyImageEntry> wallet2TxKeyImages;

  const OfflineSigningDialog({
    super.key,
    this.unsignedTxHex,
    this.fee = 0,
    required this.isViewOnly,
    this.seed,
    this.network,
    this.viewKeyHex,
    this.wallet2KeyImages = const [],
    this.wallet2TxKeyImages = const [],
  });

  static Future<TransactionSignedOfflineResponse?> show(
    BuildContext context, {
    String? unsignedTxHex,
    int fee = 0,
    required bool isViewOnly,
    String? seed,
    String? network,
    String? viewKeyHex,
    List<String> wallet2KeyImages = const [],
    List<SignedTxSetKeyImageEntry> wallet2TxKeyImages = const [],
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
        viewKeyHex: viewKeyHex,
        wallet2KeyImages: wallet2KeyImages,
        wallet2TxKeyImages: wallet2TxKeyImages,
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
  TransactionSignedOfflineResponse? _signedResponse;
  int _fee = 0;
  String? _error;
  final _importController = TextEditingController();

  StreamSubscription? _signSub;
  StreamSubscription? _signedTxSetSub;
  StreamSubscription? _builtTxSetSub;

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
    _signedTxSetSub?.cancel();
    _builtTxSetSub?.cancel();
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
      _signedResponse = null;
    });

    _signSub = TransactionSignedOfflineResponse.stream.listen((msg) {
      _signSub?.cancel();
      _signSub = null;
      if (!mounted) return;
      if (msg.success) {
        _handleSignedOfflineResponse(unsignedHex, msg);
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

  void _handleSignedOfflineResponse(
    String unsignedHex,
    TransactionSignedOfflineResponse msg,
  ) {
    if (msg.signedTxSetHex != null ||
        msg.txBlob == null ||
        !isUnsignedMoneroTxSetHex(unsignedHex) ||
        widget.wallet2KeyImages.isEmpty) {
      _showSignedResponse(msg);
      return;
    }

    final viewKeyHex = _effectiveViewKeyHex();
    if (viewKeyHex == null || viewKeyHex.isEmpty) {
      _showSignedResponse(msg);
      return;
    }

    _builtTxSetSub?.cancel();
    setState(() {
      _step = OfflineSignStep.buildingSignedTxSet;
      _error = null;
    });

    _builtTxSetSub = SignedTxSetBuiltResponse.stream.listen((built) {
      _builtTxSetSub?.cancel();
      _builtTxSetSub = null;
      if (!mounted) return;

      if (built.success && built.signedTxSetHex != null) {
        _showSignedResponse(
          _responseWithSignedTxSet(msg, built.signedTxSetHex!),
        );
        return;
      }

      _showSignedResponse(
        msg,
        warning:
            'Signed txset packaging failed; showing raw signed transaction.',
      );
    });

    BuildSignedTxSetRequest(
      unsignedTxSetHex: unsignedHex,
      viewKeyHex: viewKeyHex,
      txBlobHex: msg.txBlob!,
      keyImages: widget.wallet2KeyImages,
      txKeyImages: widget.wallet2TxKeyImages,
    ).sendSignalToRust();
  }

  TransactionSignedOfflineResponse _responseWithSignedTxSet(
    TransactionSignedOfflineResponse msg,
    String signedTxSetHex,
  ) {
    return TransactionSignedOfflineResponse(
      success: msg.success,
      error: msg.error,
      errorCode: msg.errorCode,
      errorHint: msg.errorHint,
      errorTransient: msg.errorTransient,
      txId: msg.txId,
      fee: msg.fee,
      txBlob: msg.txBlob,
      signedTxSetHex: signedTxSetHex,
      txKey: msg.txKey,
      txKeyAdditional: msg.txKeyAdditional,
      changeOutputs: msg.changeOutputs,
      spentKeyImages: msg.spentKeyImages,
    );
  }

  void _showSignedResponse(
    TransactionSignedOfflineResponse msg, {
    String? warning,
  }) {
    setState(() {
      _signedResponse = msg;
      _signedTxBlob = msg.txBlob;
      _signedTxId = msg.txId;
      _fee = msg.fee.toInt();
      _error = warning;
      _step = OfflineSignStep.showSignedQr;
    });
  }

  String? _effectiveViewKeyHex() {
    if (widget.viewKeyHex != null && widget.viewKeyHex!.isNotEmpty) {
      return widget.viewKeyHex;
    }
    return viewKeyHexFromViewOnlySeed(widget.seed);
  }

  void _handleImportedSignedHex(String hex) {
    if (hex.isEmpty) {
      setState(() => _error = 'Paste signed transaction hex');
      return;
    }
    if (isSignedMoneroTxSetHex(hex)) {
      _extractSignedTxSet(hex);
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

  void _extractSignedTxSet(String txSetHex) {
    final viewKeyHex = _effectiveViewKeyHex();
    if (viewKeyHex == null) {
      setState(() {
        _error = 'View key is required to import Monero signed txset files';
      });
      return;
    }

    _signedTxSetSub?.cancel();
    setState(() {
      _step = OfflineSignStep.extractingSignedTxSet;
      _error = null;
    });

    _signedTxSetSub = SignedTxSetExtractedResponse.stream.listen((msg) {
      _signedTxSetSub?.cancel();
      _signedTxSetSub = null;
      if (!mounted) return;

      if (!msg.success) {
        setState(() {
          _error = msg.error ?? 'Failed to import Monero signed txset';
          _step = OfflineSignStep.importSignedTx;
        });
        return;
      }

      if (msg.transactions.length != 1) {
        setState(() {
          _error =
              'Signed txset contains ${msg.transactions.length} transactions; import one transaction at a time';
          _step = OfflineSignStep.importSignedTx;
        });
        return;
      }

      final tx = msg.transactions.single;
      final spentKeyImages = msg.txKeyImages
          .map((entry) => entry.keyImage)
          .where((keyImage) => keyImage.isNotEmpty)
          .toList();
      Navigator.of(context).pop(
        TransactionSignedOfflineResponse(
          success: true,
          txId: tx.txId,
          txBlob: tx.txBlob,
          fee: tx.fee,
          txKey: tx.txKey,
          txKeyAdditional: tx.txKeyAdditional,
          changeOutputs: const [],
          spentKeyImages: spentKeyImages,
        ),
      );
    });

    ExtractSignedTxSetRequest(
      dataHex: txSetHex,
      viewKeyHex: viewKeyHex,
    ).sendSignalToRust();
  }

  String? get _signedDisplayHex =>
      _signedResponse?.signedTxSetHex ?? _signedTxBlob;

  String get _signedDownloadFilename => _signedResponse?.signedTxSetHex != null
      ? 'signed_monero_tx'
      : 'signed_tx.bin';

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
      case OfflineSignStep.buildingSignedTxSet:
        return 'Packaging Signed Transaction';
      case OfflineSignStep.extractingSignedTxSet:
        return 'Importing Signed Transaction';
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
      case OfflineSignStep.buildingSignedTxSet:
      case OfflineSignStep.extractingSignedTxSet:
        return const Center(
          child: Padding(
            padding: EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                CircularProgressIndicator(),
                SizedBox(height: 16),
                Text('Processing transaction...'),
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
                      _handleImportedSignedHex(hex);
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
                    _handleImportedSignedHex(hex);
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
        if (_signedDisplayHex != null)
          Center(
            child: AnimatedQrDisplay(
              dataHex: _signedDisplayHex!,
              urType: 'xmr-txsigned',
            ),
          ),
        const SizedBox(height: 12),
        if (_signedDisplayHex != null)
          OutlinedButton.icon(
            onPressed: () =>
                _downloadFile(_signedDisplayHex!, _signedDownloadFilename),
            icon: const Icon(Icons.download, size: 16),
            label: const Text('Download File'),
          ),
        if (_error != null) ...[
          const SizedBox(height: 8),
          Text(_error!, style: const TextStyle(color: Colors.orange)),
        ],
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
      if (_step == OfflineSignStep.showSignedQr && _signedDisplayHex != null)
        ElevatedButton(
          onPressed: () {
            Navigator.of(context).pop(
              _signedResponse ??
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
