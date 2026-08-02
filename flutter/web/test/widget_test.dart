@TestOn('browser')
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/main.dart';
import 'package:monero_extension/src/ffi/signal_sender.dart';
import 'package:monero_extension/views/debug_view.dart';

/// Browser-only: the widget tree reaches services that import `dart:js_interop`, which the
/// Dart VM has no implementation of, so `flutter test` alone cannot build this. Run with
/// `flutter test --platform chrome`.
///
/// These are smoke tests. They exist to catch "the app no longer builds or renders", which
/// is the failure a repo move or a refactor actually produces, so they assert the panels
/// are present rather than pinning the layout of a debug scaffold expected to keep moving.
class _SilentSignalSender implements SignalSender {
  @override
  void sendSignal(String signalName, Map<String, dynamic> data) {}

  @override
  Stream<Map<String, dynamic>> onRawSignal(String typeName) =>
      const Stream<Map<String, dynamic>>.empty();
}

void main() {
  setUp(() {
    // main() installs the worker bridge; without a stand-in, every widget that sends a
    // signal throws on the null sender. That, not any UI fault, is what silently broke
    // these tests at the bare-FFI refactor.
    setSignalSender(_SilentSignalSender());
  });

  Future<void> pumpApp(WidgetTester tester) async {
    await tester.pumpWidget(const AppStateHost(child: MyApp()));
    await tester.pumpAndSettle();
  }

  Future<void> pumpDebugView(WidgetTester tester) async {
    await tester.pumpWidget(
      const AppStateHost(child: MaterialApp(home: DebugView())),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('the app builds and renders', (tester) async {
    await pumpApp(tester);

    expect(tester.takeException(), isNull);
    expect(find.text('Debug View'), findsOneWidget);
  });

  testWidgets('every panel is present', (tester) async {
    await pumpDebugView(tester);

    for (final panel in const [
      'File Management',
      'Keys',
      'Receive',
      'Scanning',
      'Transactions',
      'Coins',
    ]) {
      expect(find.text(panel), findsOneWidget, reason: 'missing panel: $panel');
    }

    // Seed Phrase labels both its panel and an input; Send labels both its panel and its
    // submit button. One-or-more is the honest assertion for those.
    expect(find.text('Seed Phrase'), findsWidgets);
    expect(find.text('Send'), findsWidgets);
  });

  testWidgets('the scanning panel offers its actions', (tester) async {
    await pumpDebugView(tester);

    expect(find.text('Scan One'), findsOneWidget);
    expect(find.text('Scan All'), findsOneWidget);
    expect(find.text('Scan Mempool'), findsOneWidget);
    expect(find.text('Block Height'), findsOneWidget);
    expect(find.text('Node Address'), findsOneWidget);
  });

  testWidgets('scan controls start out usable', (tester) async {
    await pumpDebugView(tester);

    // A disabled Scan One on a fresh view would mean the panel came up in a state the user
    // cannot act from — worth pinning even though the surrounding layout is not.
    final scanOne = tester.widget<ElevatedButton>(
      find.widgetWithText(ElevatedButton, 'Scan One'),
    );
    expect(scanOne.onPressed, isNotNull);
  });

  testWidgets('the defaults point at a local stagenet node', (tester) async {
    await pumpDebugView(tester);

    expect(find.text('127.0.0.1:38081'), findsOneWidget);
    expect(find.text('Stagenet'), findsOneWidget);
  });
}
