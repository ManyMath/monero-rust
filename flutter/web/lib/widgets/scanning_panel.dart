import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../services/wallet_polling_service.dart';
import 'common_widgets.dart';

/// Widget that provides blockchain scanning controls and displays sync progress.
///
/// Includes node URL input, block height input, scan buttons (single, continuous, mempool),
/// sync progress display, and polling countdown display.
class ScanningPanel extends StatelessWidget {
  final TextEditingController nodeUrlController;
  final TextEditingController blockHeightController;
  final FocusNode blockHeightFocusNode;
  final bool isScanning;
  final bool isContinuousScanning;
  final bool isSynced;
  final bool isScanningMempool;
  final int continuousScanCurrentHeight;
  final int continuousScanTargetHeight;
  final String? scanError;
  final BlockScanResponse? scanResult;
  final bool hasSeedPhrase;
  final WalletPollingService pollingService;
  final VoidCallback onScanBlock;
  final VoidCallback onStartContinuousScan;
  final VoidCallback onPauseContinuousScan;
  final VoidCallback onScanMempool;
  final String Function() getContinuousScanButtonLabel;
  final Color Function() getContinuousScanButtonColor;

  const ScanningPanel({
    super.key,
    required this.nodeUrlController,
    required this.blockHeightController,
    required this.blockHeightFocusNode,
    required this.isScanning,
    required this.isContinuousScanning,
    required this.isSynced,
    required this.isScanningMempool,
    required this.continuousScanCurrentHeight,
    required this.continuousScanTargetHeight,
    required this.scanError,
    required this.scanResult,
    required this.hasSeedPhrase,
    required this.pollingService,
    required this.onScanBlock,
    required this.onStartContinuousScan,
    required this.onPauseContinuousScan,
    required this.onScanMempool,
    required this.getContinuousScanButtonLabel,
    required this.getContinuousScanButtonColor,
  });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            controller: nodeUrlController,
            decoration: const InputDecoration(
              labelText: 'Node Address',
              hintText: '127.0.0.1:38081',
              border: OutlineInputBorder(),
              helperText: 'For local stagenet node',
            ),
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: blockHeightController,
                  focusNode: blockHeightFocusNode,
                  decoration: const InputDecoration(
                    labelText: 'Block Height',
                    hintText: 'Block height for scan',
                    border: OutlineInputBorder(),
                  ),
                  keyboardType: TextInputType.number,
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: (isScanning || isContinuousScanning) ? null : onScanBlock,
                  icon: isScanning
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.search),
                  label: Text(isScanning ? 'Scanning...' : 'Scan One'),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: isScanning
                      ? null
                      : isContinuousScanning
                          ? onPauseContinuousScan
                          : onStartContinuousScan,
                  icon: Icon(isContinuousScanning ? Icons.pause : Icons.play_arrow),
                  label: Text(getContinuousScanButtonLabel()),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: getContinuousScanButtonColor(),
                    foregroundColor: Colors.white,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: (isScanningMempool || !hasSeedPhrase)
                      ? null
                      : onScanMempool,
                  icon: isScanningMempool
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.memory),
                  label: Text(isScanningMempool ? 'Scanning...' : 'Scan Mempool'),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: Colors.purple,
                    foregroundColor: Colors.white,
                  ),
                ),
              ),
            ],
          ),
          if (isContinuousScanning || isSynced) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: isSynced ? Colors.green.shade50 : Colors.blue.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                  color: isSynced ? Colors.green.shade200 : Colors.blue.shade200,
                ),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        isSynced ? 'Synced' : 'Scanning Progress',
                        style: TextStyle(
                          fontWeight: FontWeight.bold,
                          color: isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                        ),
                      ),
                      if (isSynced)
                        Container(
                          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                          decoration: BoxDecoration(
                            color: Colors.green,
                            borderRadius: BorderRadius.circular(12),
                          ),
                          child: const Text(
                            'SYNCED',
                            style: TextStyle(
                              color: Colors.white,
                              fontSize: 10,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  Text(
                    'Block $continuousScanCurrentHeight / $continuousScanTargetHeight',
                    style: TextStyle(
                      fontSize: 12,
                      color: isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                    ),
                  ),
                  const SizedBox(height: 8),
                  LinearProgressIndicator(
                    value: continuousScanTargetHeight > 0
                        ? continuousScanCurrentHeight / continuousScanTargetHeight
                        : 0,
                    backgroundColor: Colors.grey.shade300,
                    valueColor: AlwaysStoppedAnimation<Color>(
                      isSynced ? Colors.green : Colors.blue,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    continuousScanTargetHeight > 0
                        ? '${((continuousScanCurrentHeight / continuousScanTargetHeight) * 100).toStringAsFixed(1)}%'
                        : '0%',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.bold,
                      color: isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                    ),
                  ),
                  // Polling countdown when synced
                  if (isSynced && (pollingService.blockRefreshCountdown > 0 || pollingService.mempoolCountdown > 0)) ...[
                    const SizedBox(height: 12),
                    Text(
                      'Next poll: ${pollingService.mempoolCountdown > 0 && (pollingService.blockRefreshCountdown == 0 || pollingService.mempoolCountdown < pollingService.blockRefreshCountdown) ? pollingService.mempoolCountdown : pollingService.blockRefreshCountdown}s',
                      style: TextStyle(
                        fontSize: 11,
                        color: Colors.green.shade700,
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
          const SizedBox(height: 16),
          if (scanError != null) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.red.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.red.shade200),
              ),
              child: SelectableText(
                'Scan Error: $scanError',
                style: TextStyle(color: Colors.red.shade900),
              ),
            ),
          ],
          if (scanResult != null) ...[
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
                  CommonWidgets.buildScanResultRow(label: 'Block Height', value: scanResult!.blockHeight.toString()),
                  CommonWidgets.buildScanResultRow(label: 'Block Hash', value: scanResult!.blockHash),
                  CommonWidgets.buildScanResultRow(label: 'Timestamp', value: DateTime.fromMillisecondsSinceEpoch(
                    scanResult!.blockTimestamp.toInt() * 1000,
                  ).toString()),
                  CommonWidgets.buildScanResultRow(label: 'Transactions', value: scanResult!.txCount.toString()),
                  CommonWidgets.buildScanResultRow(label: 'Outputs Found', value: scanResult!.outputs.length.toString()),
                  if (scanResult!.outputs.isNotEmpty) ...[
                    const Divider(height: 24),
                    Text(
                      'Owned Outputs:',
                      style: TextStyle(
                        fontWeight: FontWeight.bold,
                        color: Colors.green.shade900,
                      ),
                    ),
                    const SizedBox(height: 8),
                    ...scanResult!.outputs.map((output) => Card(
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
    );
  }
}
