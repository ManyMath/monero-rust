import 'dart:async';
import 'dart:ffi' as ffi;
import 'dart:io' show Platform;
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import '../bindings/hub_native_ffi.g.dart';
import 'hub_transport.dart';

/// Native transport implementation using dart:ffi and ffigen-generated bindings.
///
/// This transport is only available on native platforms (Linux, macOS, Windows,
/// iOS, Android). It is NOT available on web/WASM where dart:ffi does not exist.
///
/// Only one instance should exist at a time (singleton pattern enforced by the
/// static [_globalController]).
class HubNativeTransport implements HubTransport {
  late final HubNativeBindings _bindings;
  late final ffi.Pointer<HubHandle> _handle;
  late final ffi.NativeCallable<RustSignalCallbackFunction> _nativeCallable;

  final StreamController<HubSignal> _controller =
      StreamController<HubSignal>.broadcast();

  // Static controller reference for the callback (since NativeCallable
  // listener callbacks must be static or top-level functions).
  static StreamController<HubSignal>? _globalController;

  @override
  Future<void> init() async {
    if (_globalController != null) {
      throw StateError('HubNativeTransport is already initialized. '
          'Only one instance may exist at a time.');
    }

    final dylib = _openLibrary();
    _bindings = HubNativeBindings(dylib);

    _nativeCallable =
        ffi.NativeCallable<RustSignalCallbackFunction>.listener(_onRustSignal);

    _globalController = _controller;

    _handle = _bindings.hub_init(
      _nativeCallable.nativeFunction,
      ffi.nullptr,
    );

    if (_handle == ffi.nullptr) {
      _globalController = null;
      _nativeCallable.close();
      throw StateError('hub_init returned null — failed to create runtime');
    }
  }

  /// Callback invoked by Rust when a RustSignal is emitted.
  static void _onRustSignal(
    int signalId,
    ffi.Pointer<ffi.Uint8> dataPtr,
    int dataLen,
    ffi.Pointer<ffi.Void> userData,
  ) {
    final copy = Uint8List.fromList(dataPtr.asTypedList(dataLen));
    _globalController?.add((signalId: signalId, data: copy));
  }

  @override
  void sendSignal(int signalId, Uint8List data) {
    if (data.isEmpty) {
      // Zero-length signals: pass a non-null dangling pointer with len=0.
      // The Rust side handles data_len==0 by creating an empty Vec.
      _bindings.hub_send_dart_signal(
        _handle,
        signalId,
        ffi.nullptr.cast<ffi.Uint8>(),
        0,
      );
      return;
    }
    final ptr = calloc<ffi.Uint8>(data.length);
    ptr.asTypedList(data.length).setAll(0, data);
    _bindings.hub_send_dart_signal(_handle, signalId, ptr, data.length);
    calloc.free(ptr);
  }

  @override
  Stream<HubSignal> get signalStream => _controller.stream;

  @override
  void shutdown() {
    _bindings.hub_shutdown(_handle);
    _nativeCallable.close();
    _globalController = null;
    _controller.close();
  }

  static ffi.DynamicLibrary _openLibrary() {
    if (Platform.isLinux || Platform.isAndroid) {
      return ffi.DynamicLibrary.open('libmonero_wasm.so');
    } else if (Platform.isMacOS || Platform.isIOS) {
      return ffi.DynamicLibrary.process();
    } else if (Platform.isWindows) {
      return ffi.DynamicLibrary.open('monero_wasm.dll');
    }
    throw UnsupportedError('Unsupported platform: ${Platform.operatingSystem}');
  }
}
