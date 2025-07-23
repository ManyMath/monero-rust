import 'dart:async';
import 'dart:typed_data';

/// A raw signal received from the Rust hub.
typedef HubSignal = ({int signalId, Uint8List data});

/// Abstract transport layer between Dart and the Rust hub.
///
/// Two implementations exist:
/// - [HubWebTransport] — wraps the WorkerBridge (Web Worker + postMessage) for web
/// - [HubNativeTransport] — uses dart:ffi (ffigen-generated bindings) for native
abstract class HubTransport {
  /// Initialize the transport (start the Rust runtime, register callbacks, etc.).
  Future<void> init();

  /// Send a DartSignal (Dart -> Rust) with the given [signalId] and
  /// JSON-serialized [data].
  void sendSignal(int signalId, Uint8List data);

  /// Stream of RustSignals (Rust -> Dart) delivered as raw bytes with signal IDs.
  Stream<HubSignal> get signalStream;

  /// Shut down the Rust runtime and release resources.
  void shutdown();
}
