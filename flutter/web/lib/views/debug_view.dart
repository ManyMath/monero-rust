import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';
import '../services/extension_service.dart';
import '../services/wallet_persistence_browser.dart';
import '../utils/clipboard_utils.dart';
import '../utils/balance_utils.dart';
import '../widgets/payment_proof_dialog.dart';
import '../widgets/keys_display_panel.dart';
import '../widgets/receive_panel.dart';
import '../widgets/scanning_panel.dart';
import '../widgets/transactions_panel.dart';
import '../widgets/outputs_panel.dart';
import '../widgets/seed_phrase_panel.dart';
import '../widgets/file_management_panel.dart';
import '../widgets/create_transaction_panel.dart';
import '../widgets/reorg_notification_display.dart';
import '../widgets/double_spend_alert_display.dart';
import '../state/app_state_scope.dart';
import '../state/output_state.dart';

enum DebugPanel {
  fileManagement('File Management'),
  seedPhrase('Seed Phrase'),
  keys('Keys'),
  receive('Receive'),
  scanning('Scanning'),
  transactions('Transactions'),
  coins('Coins'),
  createTransaction('Send');

  final String title;
  const DebugPanel(this.title);

  static DebugPanel? fromIndex(int? index) {
    if (index == null) return null;
    if (index < 0 || index >= DebugPanel.values.length) return null;
    return DebugPanel.values[index];
  }
}

class DebugView extends StatefulWidget {
  const DebugView({super.key});

  @override
  State<DebugView> createState() => _DebugViewState();
}

class _DebugViewState extends State<DebugView> {
  final _extensionService = ExtensionService();
  DebugPanel? _expandedPanel;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final ws = AppStateScope.walletOf(context);
    ws.showSnackBar = _showSnackBar;
  }

  void _toggleViewMode() {
    _extensionService.isSidePanel
        ? _extensionService.openFullPage()
        : _extensionService.openSidePanel();
  }

  void _showSnackBar(String message, {Color? backgroundColor, int seconds = 2}) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        backgroundColor: backgroundColor,
        duration: Duration(seconds: seconds),
      ),
    );
  }

  Future<void> _copyToClipboard(String text, String label) async {
    await ClipboardUtils.copyToClipboard(context, text, label);
  }

  void _navigateToTransaction(String txHash) {
    final os = AppStateScope.outputOf(context);
    setState(() {
      _expandedPanel = DebugPanel.transactions;
      os.expandedTransactions.add(txHash);
    });
    Future.delayed(const Duration(milliseconds: 100), () {
      Scrollable.ensureVisible(
        context,
        alignment: 0.5,
        duration: const Duration(milliseconds: 300),
      );
    });
  }

  ExpansionPanel _buildPanel({
    required DebugPanel panel,
    String? subtitle,
    required Widget body,
  }) {
    return ExpansionPanel(
      headerBuilder: (BuildContext context, bool isExpanded) {
        return GestureDetector(
          onTap: () {
            setState(() {
              _expandedPanel = (_expandedPanel == panel) ? null : panel;
            });
          },
          child: ListTile(
            title: Text(
              panel.title,
              style: const TextStyle(fontWeight: FontWeight.bold),
            ),
            subtitle: subtitle != null
                ? Text(subtitle, style: const TextStyle(fontSize: 12))
                : null,
          ),
        );
      },
      body: body,
      isExpanded: _expandedPanel == panel,
    );
  }

  @override
  Widget build(BuildContext context) {
    final scope = AppStateScope.of(context);
    final ws = scope.walletState;
    final os = scope.outputState;
    final ss = scope.scanState;
    final ts = scope.transactionState;
    final fs = scope.fileManagementState;

    return ListenableBuilder(
      listenable: Listenable.merge([ws, os, ss, ts, fs]),
      builder: (context, _) {
        final isSidePanel = _extensionService.isSidePanel;

        final hasData = WalletPersistenceBrowser.hasWalletData(ws.walletId);
        final totalBytes = os.calculateTotalStorageBytes();
        final fileManagementSubtitle = hasData
            ? 'Data stored: ${OutputState.formatBytes(totalBytes)}'
            : 'No stored data';

        final keyImageMap = os.keyImageMap;
        final filteredOutputs = os.filteredOutputs;
        final filteredTransactions = os.getFilteredTransactions(keyImageMap);
        final txCount = filteredTransactions.length;
        final incomingCount = filteredTransactions.where((t) => t.isIncoming(keyImageMap)).length;
        final outgoingCount = txCount - incomingCount;
        final transactionsSubtitle = txCount == 0
            ? 'No transactions'
            : '$txCount transaction${txCount == 1 ? '' : 's'} ($incomingCount in, $outgoingCount out)';

        final currentHeight = ss.currentHeight;
        final balance = BalanceUtils.calculate(filteredOutputs, currentHeight, pendingSpentKeyImages: ws.pendingSpentKeyImages);
        final coinsSubtitle = '${balance.balanceStr} - ${balance.outputCountStr}${balance.selectedStr}';

        return Scaffold(
          appBar: AppBar(
            title: const Text('Debug View'),
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
                  if (ss.reorgInfo != null)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 12),
                      child: ReorgNotificationDisplay(
                        splitHeight: ss.reorgInfo!.splitHeight.toInt(),
                        blocksDetached: ss.reorgInfo!.blocksDetached.toInt(),
                        outputsRemoved: ss.reorgInfo!.outputsRemoved.toInt(),
                        outputsUnspent: ss.reorgInfo!.outputsUnspent.toInt(),
                        onDismiss: () => setState(() => ss.reorgInfo = null),
                      ),
                    ),
                  if (ss.doubleSpendConflicts != null && ss.doubleSpendConflicts!.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 12),
                      child: DoubleSpendAlertDisplay(
                        conflicts: ss.doubleSpendConflicts!,
                        onDismiss: () => setState(() => ss.doubleSpendConflicts = null),
                      ),
                    ),
                  ExpansionPanelList(
                    expansionCallback: (int index, bool isExpanded) {
                      setState(() {
                        final panel = DebugPanel.fromIndex(index);
                        _expandedPanel = (_expandedPanel == panel) ? null : panel;
                      });
                    },
                    expandIconColor: Theme.of(context).colorScheme.primary,
                    elevation: 1,
                    expandedHeaderPadding: EdgeInsets.zero,
                    children: [
                      _buildPanel(
                        panel: DebugPanel.fileManagement,
                        subtitle: fileManagementSubtitle,
                        body: FileManagementPanel(
                          walletId: ws.walletId,
                          availableWalletIds: ws.availableWalletIds,
                          activeWallets: ws.activeWallets,
                          activeWalletId: ws.activeWalletId,
                          lastSaveTime: fs.lastSaveTime,
                          isSaving: fs.isSaving,
                          isLoadingWallet: fs.isLoadingWallet,
                          isExporting: fs.isExporting,
                          isImporting: fs.isImporting,
                          saveError: fs.saveError,
                          loadError: fs.loadError,
                          exportError: fs.exportError,
                          importError: fs.importError,
                          seed: ws.seedController.text.trim(),
                          transactionCount: ws.allTransactions.length,
                          outputCount: ws.allOutputs.length,
                          onWalletChanged: (id) => ws.switchWallet(id, loadWalletData: () => fs.loadWalletData(context)),
                          onLoad: () => fs.loadWalletData(context),
                          onDelete: () => fs.clearStoredData(context),
                          onSave: () => fs.saveWalletData(context),
                          onNew: () {
                            ss.stopPollingTimers();
                            fs.cancelAutoSave();
                            ws.startNewWallet();
                            ss.isContinuousScanning = false;
                            ss.isContinuousPaused = false;
                            ss.hasConnectedOnce = false;
                            ts.txResult = null;
                            ts.txError = null;
                            ts.broadcastResult = null;
                            ts.broadcastError = null;
                            ts.subtractFee = false;
                            fs.lastSaveTime = null;
                            fs.loadError = null;
                            fs.saveError = null;
                          },
                          onExport: () => fs.exportWallet(context),
                          onImport: () => fs.importWallet(context),
                          onCloseWallet: (id) => ws.closeWallet(
                            context,
                            id,
                            saveWalletData: () => fs.saveWalletData(context),
                            pauseScan: ss.pauseContinuousScan,
                            startScan: ss.startContinuousScan,
                            isContinuousScanning: ss.isContinuousScanning,
                          ),
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.seedPhrase,
                        body: SeedPhrasePanel(
                          controller: ws.seedController,
                          seedType: ws.seedType,
                          network: ws.network,
                          validationError: ws.validationError,
                          responseError: ws.responseError,
                          derivedLegacySeed: ws.derivedLegacySeed,
                          onGenerateSeed: ws.generateSeed,
                          onNetworkChanged: (value) {
                            ws.network = value;
                            ws.notify();
                            ws.deriveAddress();
                          },
                          onSeedTypeChanged: (value) {
                            ws.seedType = value;
                            ws.notify();
                          },
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.keys,
                        body: KeysDisplayPanel(
                          address: ws.derivedAddress,
                          secretSpendKey: ws.secretSpendKey,
                          secretViewKey: ws.secretViewKey,
                          publicSpendKey: ws.publicSpendKey,
                          publicViewKey: ws.publicViewKey,
                          network: ws.network,
                          onCopyToClipboard: _copyToClipboard,
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.receive,
                        body: ReceivePanel(
                          seed: ws.seedController.text.trim().isEmpty ? null : ws.seedController.text,
                          network: ws.network,
                          activeAccount: ws.activeAccount,
                          accounts: ws.accounts,
                          allOutputs: filteredOutputs,
                          onAccountSelected: ws.selectAccount,
                          onCreateAccount: () {
                            ws.createAccount();
                            if (ss.isContinuousScanning && ws.seedController.text.trim().isNotEmpty) {
                              ss.startContinuousScan();
                            }
                          },
                          onCopyToClipboard: _copyToClipboard,
                          subaddresses: ws.subaddresses,
                          onNavigateToTransaction: _navigateToTransaction,
                          scanningAccounts: ws.activeWallet?.scanningAccounts ?? {0},
                          onScanToggle: ws.toggleAccountScanning,
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.scanning,
                        body: ScanningPanel(
                          nodeUrlController: ws.nodeUrlController,
                          blockHeightController: ws.blockHeightController,
                          blockHeightFocusNode: ws.blockHeightFocusNode,
                          isScanning: ss.isScanning,
                          isContinuousScanning: ss.isContinuousScanning,
                          isContinuousPaused: ss.isContinuousPaused,
                          isSynced: ss.isSynced,
                          isScanningMempool: ss.isScanningMempool,
                          continuousScanCurrentHeight: ws.continuousScanCurrentHeight,
                          continuousScanTargetHeight: ss.continuousScanTargetHeight,
                          scanError: ss.scanError,
                          scanResult: ss.scanResult,
                          hasSeedPhrase: ws.seedController.text.trim().isNotEmpty,
                          pollingService: ss.pollingService,
                          restoreHeight: ws.polyseedRestoreHeight,
                          onScanBlock: ss.scanBlock,
                          onStartContinuousScan: ss.startContinuousScan,
                          onPauseContinuousScan: ss.pauseContinuousScan,
                          onScanMempool: ss.scanMempool,
                          getContinuousScanButtonLabel: ss.continuousScanButtonLabel,
                          getContinuousScanButtonColor: ss.continuousScanButtonColor,
                          connectionState: ss.connectionState,
                          lookaheadMode: ss.lookaheadMode,
                          onLookaheadModeChanged: ss.setLookaheadMode,
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.transactions,
                        subtitle: transactionsSubtitle,
                        body: TransactionsPanel(
                          allTransactions: os.sortedTransactions(filteredTransactions, keyImageMap, currentHeight),
                          keyImageMap: keyImageMap,
                          currentHeight: currentHeight,
                          txSortBy: os.txSortBy,
                          txSortAscending: os.txSortAscending,
                          expandedTransactions: os.expandedTransactions,
                          activeAccount: ws.activeAccount,
                          onSortChanged: (sortKey) {
                            if (os.txSortBy == sortKey) {
                              os.txSortAscending = !os.txSortAscending;
                            } else {
                              os.txSortBy = sortKey;
                              os.txSortAscending = false;
                            }
                            os.notify();
                          },
                          onToggleExpanded: (txHash) {
                            if (os.expandedTransactions.contains(txHash)) {
                              os.expandedTransactions.remove(txHash);
                            } else {
                              os.expandedTransactions.add(txHash);
                            }
                            os.notify();
                          },
                          onDescriptionChanged: (txHash, description) {
                            final tx = ws.allTransactions.where((t) => t.txHash == txHash).firstOrNull;
                            if (tx != null) {
                              tx.description = description.isEmpty ? null : description;
                            }
                          },
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.coins,
                        subtitle: coinsSubtitle,
                        body: OutputsPanel(
                          allOutputs: filteredOutputs,
                          currentHeight: currentHeight,
                          showSpentOutputs: os.showSpentOutputs,
                          sortBy: os.sortBy,
                          sortAscending: os.sortAscending,
                          activeAccount: ws.activeAccount,
                          onToggleShowSpent: () {
                            os.showSpentOutputs = !os.showSpentOutputs;
                            os.notify();
                          },
                          onThawAll: () {
                            final outputs = ws.allOutputs;
                            for (int i = 0; i < outputs.length; i++) {
                              if (!outputs[i].spent && outputs[i].frozen) {
                                outputs[i] = outputs[i].copyWith(frozen: false);
                                ThawOutputRequest(keyImage: outputs[i].keyImage).sendSignalToRust();
                              }
                            }
                            os.invalidateCaches();
                            ws.notify();
                          },
                          onFreezeAll: () {
                            final outputs = ws.allOutputs;
                            for (int i = 0; i < outputs.length; i++) {
                              if (!outputs[i].spent && !outputs[i].frozen) {
                                outputs[i] = outputs[i].copyWith(frozen: true);
                                FreezeOutputRequest(keyImage: outputs[i].keyImage).sendSignalToRust();
                              }
                            }
                            os.invalidateCaches();
                            ws.notify();
                          },
                          onSortChanged: (sortKey) {
                            if (os.sortBy == sortKey) {
                              os.sortAscending = !os.sortAscending;
                            } else {
                              os.sortBy = sortKey;
                              os.sortAscending = false;
                            }
                            os.notify();
                          },
                          onFreezeChanged: (keyImage, freeze) {
                            final outputs = ws.allOutputs;
                            for (int i = 0; i < outputs.length; i++) {
                              if (outputs[i].keyImage == keyImage) {
                                outputs[i] = outputs[i].copyWith(frozen: freeze);
                                break;
                              }
                            }
                            os.invalidateCaches();
                            ws.notify();
                            if (freeze) {
                              FreezeOutputRequest(keyImage: keyImage).sendSignalToRust();
                            } else {
                              ThawOutputRequest(keyImage: keyImage).sendSignalToRust();
                            }
                          },
                          pendingSpentKeyImages: ws.pendingSpentKeyImages,
                        ),
                      ),
                      _buildPanel(
                        panel: DebugPanel.createTransaction,
                        body: CreateTransactionPanel(
                          destinationControllers: ts.destinationControllers,
                          amountControllers: ts.amountControllers,
                          isCreatingTx: ts.isCreatingTx,
                          isBroadcasting: ts.isBroadcasting,
                          txResult: ts.txResult,
                          broadcastResult: ts.broadcastResult,
                          txError: ts.txError,
                          broadcastError: ts.broadcastError,
                          isBroadcastRetryable: ts.isBroadcastRetryable,
                          isBroadcastDoubleSpend: ts.isBroadcastDoubleSpend,
                          multiAccountWarning: ts.getMultiAccountWarning(),
                          onAddRecipient: ts.addRecipient,
                          onRemoveRecipient: ts.removeRecipient,
                          onCreateTransaction: ts.createTransaction,
                          onBroadcastTransaction: ts.broadcastTransaction,
                          onProvePayment: () => PaymentProofDialog.show(
                            context,
                            txResult: ts.txResult,
                            destinationControllers: ts.destinationControllers,
                            network: ws.network,
                          ),
                          onAmountChanged: () => ts.notify(),
                          onSendMax: (i) => ts.handleSendMax(context, i),
                          subtractFee: ts.subtractFee,
                          onSubtractFeeChanged: (v) {
                            ts.subtractFee = v ?? false;
                            ts.notify();
                          },
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}
