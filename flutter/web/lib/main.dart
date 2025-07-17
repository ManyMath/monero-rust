import 'src/ffi/web_ffi.dart';
import 'src/ffi/signal_dispatch.dart';
import 'package:flutter/material.dart';
import 'views/debug_view.dart';
import 'services/wallet_lifecycle_manager.dart';
import 'services/wallet_persistence_browser.dart';
import 'services/wallet_polling_service.dart';
import 'state/rinf_signal_hub.dart';
import 'state/wallet_state.dart';
import 'state/output_state.dart';
import 'state/scan_state.dart';
import 'state/transaction_state.dart';
import 'state/file_management_state.dart';
import 'state/app_state_scope.dart';

Future<void> main() async {
  await initializeBareFfi(rustSignalHandlers);
  runApp(const AppStateHost(child: MyApp()));
}

class AppStateHost extends StatefulWidget {
  final Widget child;
  const AppStateHost({super.key, required this.child});

  @override
  State<AppStateHost> createState() => _AppStateHostState();
}

class _AppStateHostState extends State<AppStateHost> {
  late final RinfSignalHub _signalHub;
  late final WalletState _walletState;
  late final OutputState _outputState;
  late final ScanState _scanState;
  late final TransactionState _transactionState;
  late final FileManagementState _fileManagementState;

  @override
  void initState() {
    super.initState();

    final lifecycle = WalletLifecycleManager(
      persistence: WalletPersistenceBrowser.defaultPersistence,
    );
    final pollingService = WalletPollingService();

    _signalHub = RinfSignalHub();

    _walletState = WalletState(
      lifecycle: lifecycle,
      signalHub: _signalHub,
    );

    _outputState = OutputState(walletState: _walletState);

    _scanState = ScanState(
      walletState: _walletState,
      pollingService: pollingService,
      signalHub: _signalHub,
    );

    _transactionState = TransactionState(
      walletState: _walletState,
      outputState: _outputState,
      scanState: _scanState,
      signalHub: _signalHub,
    );

    _fileManagementState = FileManagementState(
      walletState: _walletState,
      outputState: _outputState,
      scanState: _scanState,
    );

    _transactionState.onBroadcastSuccess = _fileManagementState.autoSaveIfReady;

    _signalHub.start();
  }

  @override
  void dispose() {
    _signalHub.dispose();
    _fileManagementState.dispose();
    _transactionState.dispose();
    _scanState.dispose();
    _outputState.dispose();
    _walletState.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AppStateScope(
      walletState: _walletState,
      outputState: _outputState,
      scanState: _scanState,
      transactionState: _transactionState,
      fileManagementState: _fileManagementState,
      child: widget.child,
    );
  }
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Monero Rust Wallet',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.orange),
        useMaterial3: true,
      ),
      home: const DebugView(),
    );
  }
}
