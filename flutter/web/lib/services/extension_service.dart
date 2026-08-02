import 'dart:js_interop';

/// The `extensionBridge` object the extension shell injects into the page.
///
/// Absent when the app runs as a plain web page rather than inside the extension, so every
/// member is nullable and callers fall back rather than assuming.
@JS('extensionBridge')
external _ExtensionBridge? get _extensionBridge;

extension type _ExtensionBridge._(JSObject _) implements JSObject {
  external bool isExtension();
  external bool isSidePanel();
  external void openFullPage();
  external void openSidePanel();
}

class ExtensionService {
  static final ExtensionService _instance = ExtensionService._internal();
  factory ExtensionService() => _instance;
  ExtensionService._internal();

  // Reading the global directly rather than off `window`: an @JS binding resolves against
  // the same global scope, and it keeps this independent of how window is typed.
  _ExtensionBridge? get _bridge => _extensionBridge;

  bool get isExtension => _bridge?.isExtension() ?? false;

  bool get isSidePanel => _bridge?.isSidePanel() ?? false;

  Future<void> openFullPage() async => _bridge?.openFullPage();

  Future<void> openSidePanel() async => _bridge?.openSidePanel();
}
