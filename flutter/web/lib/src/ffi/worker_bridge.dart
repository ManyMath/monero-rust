import 'package:web/web.dart' as web;
import 'dart:js_interop';
import 'dart:async';
import 'dart:convert';
import 'signal_sender.dart';
import '../logging.dart';

class WorkerBridge implements SignalSender {
  static const _tag = 'WorkerBridge';

  late final web.Worker _worker;
  final _readyCompleter = Completer<void>();
  final _signalControllers = <String, StreamController<Map<String, dynamic>>>{};

  Future<void> init() async {
    Log.info(_tag, 'Initializing Web Worker');
    final opts = web.WorkerOptions(type: 'module');
    _worker = web.Worker('pkg/monero_wasm_worker.js'.toJS, opts);
    _worker.onmessage = _onMessage.toJS;
    _worker.onerror = _onError.toJS;
    await _readyCompleter.future;
    Log.info(_tag, 'Web Worker ready');
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
      Log.error(_tag, 'Worker error for $signalName: $error');
      return;
    }
  }

  void _onError(web.Event event) {
    Log.error(_tag, 'Worker error: $event');
    if (!_readyCompleter.isCompleted) {
      _readyCompleter.completeError('Worker failed to initialize');
    }
  }

  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {
    Log.debug(_tag, 'Sending signal: $signalName');
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
    Log.info(_tag, 'Disposing Web Worker');
    _worker.terminate();
    for (final c in _signalControllers.values) {
      c.close();
    }
    _signalControllers.clear();
  }
}
