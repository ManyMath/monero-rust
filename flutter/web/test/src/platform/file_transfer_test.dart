import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/src/platform/file_transfer.dart';

import 'fake_file_transfer.dart';

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

    expect(await fileTransfer.pick(), isNull);
    expect(fake.pickCalls, 1);
  });

  test('a picked file comes back verbatim, name and all', () async {
    final fake = FakeFileTransfer()
      ..nextPick = (name: 'signed.bin', bytes: const [0xde, 0xad, 0xbe, 0xef]);
    setFileTransfer(fake);

    final picked = await fileTransfer.pick();

    expect(picked!.name, 'signed.bin');
    expect(picked.bytes, const [0xde, 0xad, 0xbe, 0xef]);
  });

  test('the caller chooses which files the picker offers', () async {
    final fake = FakeFileTransfer();
    setFileTransfer(fake);

    await fileTransfer.pick(accept: '.keys,*');

    expect(fake.acceptFilters, const ['.keys,*']);
  });
}
