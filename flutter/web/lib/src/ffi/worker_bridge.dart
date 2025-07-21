import 'package:web/web.dart' as web;
import 'dart:js_interop';
import 'dart:async';
import 'dart:convert';
import 'signal_sender.dart';

class WorkerBridge implements SignalSender {
  late final web.Worker _worker;
  final _readyCompleter = Completer<void>();
  final _signalControllers = <String, StreamController<Map<String, dynamic>>>{};

  Future<void> init() async {
    final opts = web.WorkerOptions(type: 'module');
    _worker = web.Worker('pkg/monero_wasm_worker.js'.toJS, opts);
    _worker.onmessage = _onMessage.toJS;
    _worker.onerror = _onError.toJS;
    await _readyCompleter.future;
  }

  void _onMessage(web.MessageEvent event) {
    final data = (event.data as JSAny).dartify() as Map;
    final type = data['type'] as String;

    if (type == 'ready') {
      if (!_readyCompleter.isCompleted) {
        _readyCompleter.complete();
      }
      return;
    }

    if (type == 'rust_signal') {
      final typeName = data['typeName'] as String;
      final json = data['json'] as String;
      final controller = _signalControllers[typeName];
      if (controller != null) {
        final map = jsonDecode(json) as Map<String, dynamic>;
        controller.add(map);
      }
      return;
    }

    if (type == 'error') {
      final signalName = data['signalName'] as String;
      final error = data['error'] as String;
      // ignore: avoid_print
      print('Worker error for $signalName: $error');
      return;
    }
  }

  void _onError(web.Event event) {
    // ignore: avoid_print
    print('Worker error: $event');
    if (!_readyCompleter.isCompleted) {
      _readyCompleter.completeError('Worker failed to initialize');
    }
  }

  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {
    final msg = {
      'type': 'dart_signal',
      'signalName': signalName,
      'json': jsonEncode(data),
    }.jsify();
    _worker.postMessage(msg);
  }

  @override
  Stream<Map<String, dynamic>> onRawSignal(String typeName) {
    return _signalControllers
        .putIfAbsent(typeName, () => StreamController.broadcast())
        .stream;
  }

  void dispose() {
    _worker.terminate();
    for (final c in _signalControllers.values) {
      c.close();
    }
    _signalControllers.clear();
  }
}
