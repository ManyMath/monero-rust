import 'dart:async';
import 'dart:convert';
import 'dart:html' as html;

import 'file_transfer.dart';

/// Browser file transfer, via an anchor click to send and a file input to receive.
///
/// This is the only place `dart:html` is reached for on the offline signing path; keeping
/// it in a leaf file is what lets the dialog itself be tested on the Dart VM.
class WebFileTransfer implements FileTransfer {
  const WebFileTransfer();

  @override
  void download(String filename, List<int> bytes) {
    final blob = html.Blob([bytes], 'application/octet-stream');
    final url = html.Url.createObjectUrlFromBlob(blob);
    html.AnchorElement(href: url)
      ..setAttribute('download', filename)
      ..click();
    html.Url.revokeObjectUrl(url);
  }

  @override
  Future<List<int>?> pickAndRead({
    Duration timeout = const Duration(seconds: 120),
  }) async {
    final upload = html.FileUploadInputElement();
    upload.accept = '.bin,*';
    upload.click();
    try {
      await upload.onChange.first.timeout(timeout);
    } on TimeoutException {
      return null;
    }

    final files = upload.files;
    if (files == null || files.isEmpty) return null;

    final reader = html.FileReader();
    reader.readAsDataUrl(files[0]);
    await reader.onLoad.first;
    final dataUrl = reader.result as String;
    return base64Decode(dataUrl.substring(dataUrl.indexOf(',') + 1));
  }
}

void setWebFileTransfer() => setFileTransfer(const WebFileTransfer());
