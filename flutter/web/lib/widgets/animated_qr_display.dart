import 'dart:async';
import 'package:flutter/material.dart';
import '../src/ffi/signal_types.dart';

class AnimatedQrDisplay extends StatefulWidget {
  final String dataHex;
  final String urType;
  final int maxFragmentLen;

  const AnimatedQrDisplay({
    super.key,
    required this.dataHex,
    required this.urType,
    this.maxFragmentLen = 150,
  });

  @override
  State<AnimatedQrDisplay> createState() => _AnimatedQrDisplayState();
}

class _AnimatedQrDisplayState extends State<AnimatedQrDisplay> {
  StreamSubscription? _frameSub;
  List<bool>? _modules;
  int _size = 0;
  int _seqNum = 0;
  int _seqLen = 0;

  @override
  void initState() {
    super.initState();
    _frameSub = QrFrameResponse.stream.listen((f) {
      if (!mounted) return;
      setState(() {
        _modules = f.modules;
        _size = f.size;
        _seqNum = f.seqNum;
        _seqLen = f.seqLen;
      });
    });
    _startEncoder();
  }

  void _startEncoder() {
    StartUrEncoderRequest(
      dataHex: widget.dataHex,
      urType: widget.urType,
      maxFragmentLen: widget.maxFragmentLen,
    ).sendSignalToRust();
  }

  void _stopEncoder() {
    const StopUrEncoderRequest().sendSignalToRust();
  }

  @override
  void didUpdateWidget(AnimatedQrDisplay old) {
    super.didUpdateWidget(old);
    if (old.dataHex != widget.dataHex || old.urType != widget.urType) {
      _startEncoder();
    }
  }

  @override
  void dispose() {
    _stopEncoder();
    _frameSub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_modules == null || _size == 0) {
      return const SizedBox(
        width: 250,
        height: 250,
        child: Center(child: CircularProgressIndicator()),
      );
    }

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          padding: const EdgeInsets.all(8),
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(4),
          ),
          child: CustomPaint(
            size: const Size(250, 250),
            painter: _QrPainter(modules: _modules!, size: _size),
          ),
        ),
        if (_seqLen > 1)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              'Part $_seqNum / $_seqLen',
              style: const TextStyle(fontSize: 12, color: Colors.grey),
            ),
          ),
      ],
    );
  }
}

class _QrPainter extends CustomPainter {
  final List<bool> modules;
  final int size;

  _QrPainter({required this.modules, required this.size});

  @override
  void paint(Canvas canvas, Size canvasSize) {
    if (size == 0) return;
    final cellSize = canvasSize.width / size;
    final darkPaint = Paint()..color = Colors.black;

    for (int y = 0; y < size; y++) {
      for (int x = 0; x < size; x++) {
        if (modules[y * size + x]) {
          canvas.drawRect(
            Rect.fromLTWH(x * cellSize, y * cellSize, cellSize, cellSize),
            darkPaint,
          );
        }
      }
    }
  }

  @override
  bool shouldRepaint(_QrPainter old) => true;
}
