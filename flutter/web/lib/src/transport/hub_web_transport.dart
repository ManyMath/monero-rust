import 'dart:async';
import 'dart:typed_data';

import 'hub_transport.dart';

/// Web transport implementation that wraps the WorkerBridge.
///
/// This is the PRIMARY transport used for the web/WASM target. The actual
/// communication happens via WorkerBridge (postMessage to a Web Worker).
///
/// On web, the existing typed signal classes (e.g. `FreezeOutputRequest.send()`
/// and `FreezeThawResponse.stream`) are the primary API. The [sendSignal] and
/// [signalStream] methods on this class are provided for API compatibility with
/// [HubTransport] but are not the primary send/receive path on web.
class HubWebTransport implements HubTransport {
  final StreamController<HubSignal> _controller =
      StreamController<HubSignal>.broadcast();

  /// Web transport initialization is handled by WorkerBridge in the app layer.
  @override
  Future<void> init() async {}

  /// Not used on web — signals are sent via typed `.send()` methods through
  /// WorkerBridge. This is a no-op on the web path.
  @override
  void sendSignal(int signalId, Uint8List data) {}

  @override
  Stream<HubSignal> get signalStream => _controller.stream;

  @override
  void shutdown() {
    _controller.close();
  }
}
