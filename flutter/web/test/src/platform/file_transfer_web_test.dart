@TestOn('browser')
library;

import 'dart:convert';
import 'dart:html' as html;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/src/platform/file_transfer_web.dart';
import 'package:monero_extension/src/platform/web_blob.dart';

/// Covers the `dart:html` leaf of the offline signing path, which the `FileTransfer` seam
/// deliberately cannot reach.
///
/// `pick()` still isn't here — it opens a real file dialog and waits for a human. What is
/// here is the half that decides what a downloaded file *contains*, which is where the one
/// silent failure lives: a Blob built from a plain `List<int>` holds the decimal text of
/// the bytes rather than the bytes, and nothing anywhere throws. Exports are the last step
/// of offline signing, so a corrupt one is discovered by the machine that was supposed to
/// sign it.
Future<Uint8List> bytesOf(html.Blob blob) async {
  final reader = html.FileReader();
  reader.readAsArrayBuffer(blob);
  await reader.onLoad.first;
  final result = reader.result;
  return result is Uint8List ? result : (result as ByteBuffer).asUint8List();
}

void main() {
  group('binaryBlob', () {
    // Every byte here is one the stringifying path mangles: NUL and 0xFF stop being one
    // byte, and the comma and newline are what it would join the rest with.
    final payload = <int>[0x00, 0x01, 0x7f, 0x80, 0xff, 0x2c, 0x0a];

    test('a plain List<int> reaches the file as bytes, not as its decimal text', () async {
      // Deliberately not a Uint8List. Passing one is what the two callers happen to do
      // today, so testing with one would pass against the broken code as well.
      final blob = binaryBlob(payload);

      expect(
        blob.size,
        payload.length,
        reason: 'a stringified Array would be 21 bytes of "0,1,127,128,255,44,10"',
      );
      expect(await bytesOf(blob), payload);
    });

    test('a Uint8List is passed through unchanged', () async {
      final blob = binaryBlob(Uint8List.fromList(payload));
      expect(await bytesOf(blob), payload);
    });

    test('utf8-encoded text survives, including multi-byte characters', () async {
      final encoded = utf8.encode('héllo wörld');
      final blob = binaryBlob(encoded);
      expect(utf8.decode(await bytesOf(blob)), 'héllo wörld');
    });

    test('an empty payload makes an empty blob rather than throwing', () async {
      expect(binaryBlob(const <int>[]).size, 0);
    });
  });

  group('WebFileTransfer', () {
    test('download() completes against the real dart:html APIs', () {
      // Whether the browser writes the file is the browser's business and not observable
      // here. What this pins is that the four calls download() makes — Blob, object URL,
      // anchor, revoke — are all still valid, which is the part that rots silently when
      // `dart:html` changes underneath it.
      expect(
        () => const WebFileTransfer().download('unsigned.bin', <int>[1, 2, 3]),
        returnsNormally,
      );
    });
  });
}
