import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/src/platform/file_transfer.dart';

/// Records what the offline signing flow hands out and hands back.
class FakeFileTransfer implements FileTransfer {
  final downloads = <({String filename, List<int> bytes})>[];
  List<int>? nextPick;
  int pickCalls = 0;

  @override
  void download(String filename, List<int> bytes) {
    downloads.add((filename: filename, bytes: bytes));
  }

  @override
  Future<List<int>?> pickAndRead({Duration timeout = const Duration(seconds: 120)}) async {
    pickCalls++;
    return nextPick;
  }
}

void main() {
  test('an uninstalled transfer throws rather than silently doing nothing', () {
    clearFileTransfer();

    // A no-op default would be indistinguishable from the user cancelling a save, so
    // "your signed transaction vanished" would be the first sign of a wiring mistake.
    expect(() => fileTransfer, throwsA(isA<StateError>()));
  });

  test('the installed transfer is the one used', () {
    final fake = FakeFileTransfer();
    setFileTransfer(fake);

    fileTransfer.download('unsigned_monero_tx', const [1, 2, 3]);

    expect(fake.downloads, hasLength(1));
    expect(fake.downloads.single.filename, 'unsigned_monero_tx');
    expect(fake.downloads.single.bytes, const [1, 2, 3]);
  });

  test('a cancelled pick reads as null rather than empty bytes', () async {
    final fake = FakeFileTransfer()..nextPick = null;
    setFileTransfer(fake);

    expect(await fileTransfer.pickAndRead(), isNull);
    expect(fake.pickCalls, 1);
  });

  test('a picked file comes back verbatim', () async {
    final fake = FakeFileTransfer()..nextPick = const [0xde, 0xad, 0xbe, 0xef];
    setFileTransfer(fake);

    expect(await fileTransfer.pickAndRead(), const [0xde, 0xad, 0xbe, 0xef]);
  });
}
