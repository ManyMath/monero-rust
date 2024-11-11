import 'package:rinf/rinf.dart';
import 'src/bindings/bindings.dart';
import 'package:flutter/material.dart';
import 'views/debug_view.dart';

Future<void> main() async {
  await initializeRust(assignRustSignal);
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'monero-wasm',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.orange),
        useMaterial3: true,
      ),
      home: const DebugView(),
    );
  }
}

class TestPage extends StatefulWidget {
  const TestPage({super.key});

  @override
  State<TestPage> createState() => _TestPageState();
}

class _TestPageState extends State<TestPage> {
  String _result = 'Press button to test';

  @override
  void initState() {
    super.initState();
    MoneroTestResponse.rustSignalStream.listen((signal) {
      setState(() {
        _result = signal.message.result;
      });
    });
  }

  void _test() {
    MoneroTestRequest().sendSignalToRust();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Monero WASM Test'),
      ),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(_result),
            const SizedBox(height: 20),
            ElevatedButton(
              onPressed: _test,
              child: const Text('Test monero-wasm'),
            ),
          ],
        ),
      ),
    );
  }
}
