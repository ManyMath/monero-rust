import 'dart:js_util' as js_util;
import 'dart:html' as html;

class ExtensionService {
  static final ExtensionService _instance = ExtensionService._internal();
  factory ExtensionService() => _instance;
  ExtensionService._internal();

  Object? get _bridge => js_util.getProperty(html.window, 'extensionBridge');

  bool get isExtension {
    final bridge = _bridge;
    if (bridge == null) return false;
    return js_util.callMethod<bool>(bridge, 'isExtension', []);
  }

  bool get isSidePanel {
    final bridge = _bridge;
    if (bridge == null) return false;
    return js_util.callMethod<bool>(bridge, 'isSidePanel', []);
  }

  Future<void> openFullPage() async {
    final bridge = _bridge;
    if (bridge == null) return;
    js_util.callMethod(bridge, 'openFullPage', []);
  }

  Future<void> openSidePanel() async {
    final bridge = _bridge;
    if (bridge == null) return;
    js_util.callMethod(bridge, 'openSidePanel', []);
  }
}
