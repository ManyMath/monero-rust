import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/main.dart';
import 'package:monero_extension/views/debug_view.dart';

void main() {
  testWidgets('Main app smoke test', (WidgetTester tester) async {
    // Build our app and trigger a frame.
    await tester.pumpWidget(const MyApp());

    // Verify app title
    expect(find.text('Monero Wallet Debug'), findsOneWidget);
  });

  testWidgets('DebugView has scan buttons', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Find the scanning expansion panel and expand it
    final scanningPanel = find.text('Scanning');
    expect(scanningPanel, findsOneWidget);

    // Tap to expand
    await tester.tap(scanningPanel);
    await tester.pumpAndSettle();

    // Verify both scan buttons exist
    expect(find.text('Scan One'), findsOneWidget);
    expect(find.text('Start Scan'), findsOneWidget);
  });

  testWidgets('Scan One button is initially enabled', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Expand scanning panel
    await tester.tap(find.text('Scanning'));
    await tester.pumpAndSettle();

    // Find Scan One button
    final scanOneButton = find.widgetWithText(ElevatedButton, 'Scan One');
    expect(scanOneButton, findsOneWidget);

    // Button should be enabled (can be tapped)
    final button = tester.widget<ElevatedButton>(scanOneButton);
    expect(button.onPressed, isNotNull);
  });

  testWidgets('Start Scan button is initially enabled', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Expand scanning panel
    await tester.tap(find.text('Scanning'));
    await tester.pumpAndSettle();

    // Find Start Scan button
    final startScanButton = find.widgetWithText(ElevatedButton, 'Start Scan');
    expect(startScanButton, findsOneWidget);

    // Button should be enabled
    final button = tester.widget<ElevatedButton>(startScanButton);
    expect(button.onPressed, isNotNull);
  });

  testWidgets('Block Height text field exists', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Expand scanning panel
    await tester.tap(find.text('Scanning'));
    await tester.pumpAndSettle();

    // Find Block Height field
    final blockHeightField = find.widgetWithText(TextField, 'Block Height');
    expect(blockHeightField, findsOneWidget);
  });

  testWidgets('Node URL field has default value', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Expand scanning panel
    await tester.tap(find.text('Scanning'));
    await tester.pumpAndSettle();

    // Find Node URL field
    final nodeUrlField = find.widgetWithText(TextField, '127.0.0.1:38081');
    expect(nodeUrlField, findsOneWidget);
  });

  testWidgets('Network dropdown exists', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Expand scanning panel
    await tester.tap(find.text('Scanning'));
    await tester.pumpAndSettle();

    // Find network dropdown with default value
    expect(find.text('stagenet'), findsWidgets);
  });

  testWidgets('Seed phrase panel exists', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Find seed phrase panel
    expect(find.text('Seed Phrase'), findsOneWidget);
  });

  testWidgets('Coins panel exists', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Find coins panel
    expect(find.text('Coins'), findsOneWidget);
  });

  testWidgets('Transaction panel exists', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: DebugView()));
    await tester.pumpAndSettle();

    // Find transaction panel
    expect(find.text('Transaction'), findsOneWidget);
  });
}
