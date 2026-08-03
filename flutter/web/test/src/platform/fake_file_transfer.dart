import 'package:monero_extension/src/platform/file_transfer.dart';

/// Records what the app hands out, and hands back whatever the test queued.
///
/// Shared so the offline signing flow and the file management state are checked against the
/// same fake; two fakes drifting apart would let one of them pass on a contract the real
/// [WebFileTransfer] no longer honours.
class FakeFileTransfer implements FileTransfer {
  final downloads = <({String filename, List<int> bytes})>[];
  final acceptFilters = <String>[];
  PickedFile? nextPick;
  int pickCalls = 0;

  @override
  void download(String filename, List<int> bytes) {
    downloads.add((filename: filename, bytes: bytes));
  }

  @override
  Future<PickedFile?> pick({
    String accept = '.bin,*',
    Duration timeout = const Duration(seconds: 120),
  }) async {
    pickCalls++;
    acceptFilters.add(accept);
    return nextPick;
  }
}
