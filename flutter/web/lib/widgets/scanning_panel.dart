import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../services/wallet_polling_service.dart';
import '../state/scan_state.dart' show LookaheadMode;
import 'common_widgets.dart';

enum NodeConnectionState {
  disconnected,
  connecting,
  synchronizing,
  synchronized;

  String get label => switch (this) {
    disconnected => 'Disconnected',
    connecting => 'Connecting',
    synchronizing => 'Synchronizing',
    synchronized => 'Synchronized',
  };

  Color get color => switch (this) {
    disconnected => Colors.red,
    connecting => Colors.orange,
    synchronizing => Colors.yellow.shade700,
    synchronized => Colors.green,
  };

  IconData get icon => switch (this) {
    disconnected => Icons.cloud_off,
    connecting => Icons.cloud_queue,
    synchronizing => Icons.sync,
    synchronized => Icons.cloud_done,
  };
}

class ScanningPanel extends StatefulWidget {
  final TextEditingController nodeUrlController;
  final TextEditingController blockHeightController;
  final FocusNode blockHeightFocusNode;
  final bool isScanning;
  final bool isContinuousScanning;
  final bool isContinuousPaused;
  final bool isSynced;
  final bool isScanningMempool;
  final int continuousScanCurrentHeight;
  final int continuousScanTargetHeight;
  final String? scanError;
  final BlockScanResponse? scanResult;
  final bool hasSeedPhrase;
  final WalletPollingService pollingService;
  final int? restoreHeight;
  final VoidCallback onScanBlock;
  final VoidCallback onStartContinuousScan;
  final VoidCallback onPauseContinuousScan;
  final VoidCallback onScanMempool;
  final String Function() getContinuousScanButtonLabel;
  final Color Function() getContinuousScanButtonColor;
  final NodeConnectionState connectionState;
  final LookaheadMode lookaheadMode;
  final ValueChanged<LookaheadMode> onLookaheadModeChanged;

  const ScanningPanel({
    super.key,
    required this.nodeUrlController,
    required this.blockHeightController,
    required this.blockHeightFocusNode,
    required this.isScanning,
    required this.isContinuousScanning,
    required this.isContinuousPaused,
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
    required this.lookaheadMode,
    required this.onLookaheadModeChanged,
    this.restoreHeight,
    this.connectionState = NodeConnectionState.disconnected,
  });

  @override
  State<ScanningPanel> createState() => _ScanningPanelState();
}

class _ScanningPanelState extends State<ScanningPanel> {

  Widget _buildCountdownRow({
    required ValueNotifier<int> blockNotifier,
    required ValueNotifier<int> mempoolNotifier,
    required Color textColor,
    bool showIcons = false,
  }) {
    return ValueListenableBuilder<int>(
      valueListenable: blockNotifier,
      builder: (context, blockVal, _) {
        return ValueListenableBuilder<int>(
          valueListenable: mempoolNotifier,
          builder: (context, mempoolVal, _) {
            if (blockVal <= 0 && mempoolVal <= 0) return const SizedBox.shrink();
            return Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                if (blockVal > 0)
                  Row(
                    children: [
                      if (showIcons) ...[
                        Icon(Icons.timer, size: 16, color: textColor),
                        const SizedBox(width: 4),
                      ],
                      Text(
                        'Next block poll: ${blockVal}s',
                        style: TextStyle(
                          fontSize: showIcons ? 12 : 11,
                          color: textColor,
                        ),
                      ),
                    ],
                  ),
                if (mempoolVal > 0)
                  Row(
                    children: [
                      if (showIcons) ...[
                        Icon(Icons.memory, size: 16, color: textColor),
                        const SizedBox(width: 4),
                      ],
                      Text(
                        'Next mempool poll: ${mempoolVal}s',
                        style: TextStyle(
                          fontSize: showIcons ? 12 : 11,
                          color: textColor,
                        ),
                      ),
                    ],
                  ),
              ],
            );
          },
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: widget.nodeUrlController,
                  decoration: const InputDecoration(
                    labelText: 'Node Address',
                    hintText: '127.0.0.1:38081',
                    border: OutlineInputBorder(),
                    helperText: 'For local stagenet node',
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                decoration: BoxDecoration(
                  color: widget.connectionState.color.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(4),
                  border: Border.all(color: widget.connectionState.color),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(widget.connectionState.icon, size: 14, color: widget.connectionState.color),
                    const SizedBox(width: 4),
                    Text(
                      widget.connectionState.label,
                      style: TextStyle(fontSize: 11, color: widget.connectionState.color, fontWeight: FontWeight.bold),
                    ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          // First row: Block height input and Lookahead dropdown
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: widget.blockHeightController,
                  focusNode: widget.blockHeightFocusNode,
                  decoration: const InputDecoration(
                    labelText: 'Block Height',
                    hintText: 'Block height for scan',
                    border: OutlineInputBorder(),
                  ),
                  keyboardType: TextInputType.number,
                ),
              ),
              const SizedBox(width: 16),
              Expanded(
                child: DropdownButtonFormField<LookaheadMode>(
                  value: widget.lookaheadMode,
                  decoration: const InputDecoration(
                    labelText: 'Lookahead',
                    border: OutlineInputBorder(),
                  ),
                  items: LookaheadMode.values
                      .map((mode) => DropdownMenuItem(
                            value: mode,
                            child: Text(mode.label),
                          ))
                      .toList(),
                  onChanged: (value) {
                    if (value != null) {
                      widget.onLookaheadModeChanged(value);
                    }
                  },
                ),
              ),
            ],
          ),
          if (widget.restoreHeight != null) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.blue.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.blue.shade200),
              ),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Polyseed Restore Height',
                          style: TextStyle(
                            fontWeight: FontWeight.bold,
                            color: Colors.blue.shade900,
                            fontSize: 12,
                          ),
                        ),
                        const SizedBox(height: 4),
                        SelectableText(
                          widget.restoreHeight.toString(),
                          style: TextStyle(
                            fontSize: 14,
                            fontFamily: 'monospace',
                            color: Colors.blue.shade900,
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 12),
                  Text(
                    '(from polyseed birthday)',
                    style: TextStyle(
                      fontSize: 11,
                      color: Colors.blue.shade700,
                      fontStyle: FontStyle.italic,
                    ),
                  ),
                ],
              ),
            ),
          ],
          const SizedBox(height: 16),
          // Second row: Scan buttons
          Row(
            children: [
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: (widget.isScanning || widget.isContinuousScanning) ? null : widget.onScanBlock,
                  icon: widget.isScanning
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.search),
                  label: Text(widget.isScanning ? 'Scanning...' : 'Scan One'),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: widget.isScanning
                      ? null
                      : widget.isContinuousScanning
                          ? widget.onPauseContinuousScan
                          : widget.onStartContinuousScan,
                  icon: Icon(widget.isContinuousScanning ? Icons.pause : Icons.play_arrow),
                  label: Text(widget.isContinuousScanning ? 'Pause Scan' : 'Scan All'),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: widget.getContinuousScanButtonColor(),
                    foregroundColor: Colors.white,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: (widget.isScanningMempool || !widget.hasSeedPhrase)
                      ? null
                      : widget.onScanMempool,
                  icon: widget.isScanningMempool
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.memory),
                  label: Text(widget.isScanningMempool ? 'Scanning...' : 'Scan Mempool'),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: Colors.purple,
                    foregroundColor: Colors.white,
                  ),
                ),
              ),
            ],
          ),
          // Standalone polling countdown (not scanning, not paused, not synced)
          if (!widget.isContinuousScanning && !widget.isContinuousPaused && !widget.isSynced) ...[
            ValueListenableBuilder<int>(
              valueListenable: widget.pollingService.blockRefreshCountdown,
              builder: (context, blockVal, _) {
                return ValueListenableBuilder<int>(
                  valueListenable: widget.pollingService.mempoolCountdown,
                  builder: (context, mempoolVal, _) {
                    if (blockVal <= 0 && mempoolVal <= 0) return const SizedBox.shrink();
                    return Padding(
                      padding: const EdgeInsets.only(top: 16),
                      child: Container(
                        padding: const EdgeInsets.all(12),
                        decoration: BoxDecoration(
                          color: Colors.grey.shade50,
                          borderRadius: BorderRadius.circular(8),
                          border: Border.all(color: Colors.grey.shade300),
                        ),
                        child: _buildCountdownRow(
                          blockNotifier: widget.pollingService.blockRefreshCountdown,
                          mempoolNotifier: widget.pollingService.mempoolCountdown,
                          textColor: Colors.grey.shade700,
                          showIcons: true,
                        ),
                      ),
                    );
                  },
                );
              },
            ),
          ],
          if (widget.isContinuousScanning || widget.isSynced) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: widget.isSynced ? Colors.green.shade50 : Colors.blue.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                  color: widget.isSynced ? Colors.green.shade200 : Colors.blue.shade200,
                ),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        widget.isSynced ? 'Synced' : 'Scanning Progress',
                        style: TextStyle(
                          fontWeight: FontWeight.bold,
                          color: widget.isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                        ),
                      ),
                      if (widget.isSynced)
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
                    'Block ${widget.continuousScanCurrentHeight} / ${widget.continuousScanTargetHeight}',
                    style: TextStyle(
                      fontSize: 12,
                      color: widget.isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                    ),
                  ),
                  const SizedBox(height: 8),
                  LinearProgressIndicator(
                    value: widget.continuousScanTargetHeight > 0
                        ? widget.continuousScanCurrentHeight / widget.continuousScanTargetHeight
                        : 0,
                    backgroundColor: Colors.grey.shade300,
                    valueColor: AlwaysStoppedAnimation<Color>(
                      widget.isSynced ? Colors.green : Colors.blue,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    widget.continuousScanTargetHeight > 0
                        ? '${((widget.continuousScanCurrentHeight / widget.continuousScanTargetHeight) * 100).toStringAsFixed(1)}%'
                        : '0%',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.bold,
                      color: widget.isSynced ? Colors.green.shade900 : Colors.blue.shade900,
                    ),
                  ),
                  // Polling countdown timer display (inside sync panel)
                  const SizedBox(height: 12),
                  _buildCountdownRow(
                    blockNotifier: widget.pollingService.blockRefreshCountdown,
                    mempoolNotifier: widget.pollingService.mempoolCountdown,
                    textColor: widget.isSynced ? Colors.green.shade700 : Colors.blue.shade700,
                  ),
                ],
              ),
            ),
          ],
          const SizedBox(height: 16),
          if (widget.scanError != null) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.red.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.red.shade200),
              ),
              child: SelectableText(
                'Scan Error: ${widget.scanError}',
                style: TextStyle(color: Colors.red.shade900),
              ),
            ),
          ],
          if (widget.scanResult != null) ...[
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
                  CommonWidgets.buildScanResultRow(label: 'Block Height', value: widget.scanResult!.blockHeight.toString()),
                  CommonWidgets.buildScanResultRow(label: 'Block Hash', value: widget.scanResult!.blockHash),
                  CommonWidgets.buildScanResultRow(label: 'Timestamp', value: DateTime.fromMillisecondsSinceEpoch(
                    widget.scanResult!.blockTimestamp.toInt() * 1000,
                  ).toString()),
                  CommonWidgets.buildScanResultRow(label: 'Transactions', value: widget.scanResult!.txCount.toString()),
                  CommonWidgets.buildScanResultRow(label: 'Outputs Found', value: widget.scanResult!.outputs.length.toString()),
                  if (widget.scanResult!.outputs.isNotEmpty) ...[
                    const Divider(height: 24),
                    Text(
                      'Owned Outputs:',
                      style: TextStyle(
                        fontWeight: FontWeight.bold,
                        color: Colors.green.shade900,
                      ),
                    ),
                    const SizedBox(height: 8),
                    ...widget.scanResult!.outputs.map((output) => Card(
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
