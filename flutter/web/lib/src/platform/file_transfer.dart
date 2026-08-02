import 'dart:async';

/// Moving a file to and from the user, without naming the platform that does it.
///
/// The offline signing flow hands unsigned transactions out and takes signed ones back, and
/// on web that means `dart:html` — a library the Dart VM does not have, so importing it
/// anywhere in a widget's tree makes that widget untestable under `flutter test`. Keeping
/// it behind this interface is the same trick [SignalSender] uses for the worker bridge.
abstract class FileTransfer {
  /// Offer `bytes` to the user as a download named `filename`.
  void download(String filename, List<int> bytes);

  /// Ask the user for a file and return its contents, or null if they didn't pick one.
  Future<List<int>?> pickAndRead({Duration timeout});
}

FileTransfer? _fileTransfer;

/// The transfer implementation for this platform.
///
/// Throws if nothing was installed, which is deliberate: a silent no-op would look like the
/// user cancelling, and "your signed transaction vanished" is a bad way to learn about a
/// wiring mistake.
FileTransfer get fileTransfer {
  final transfer = _fileTransfer;
  if (transfer == null) {
    throw StateError(
      'No FileTransfer installed. Call setFileTransfer() during startup — '
      'setWebFileTransfer() on web, or a fake in tests.',
    );
  }
  return transfer;
}

void setFileTransfer(FileTransfer transfer) => _fileTransfer = transfer;

/// Forget the installed transfer, so a test can prove the unset case still throws.
void clearFileTransfer() => _fileTransfer = null;
