import 'dart:async';
import 'dart:convert';
import 'dart:html' as html;

import 'file_transfer.dart';
import 'web_blob.dart';

/// Browser file transfer, via an anchor click to send and a file input to receive.
///
/// `dart:html` on the offline signing path is confined to this file and `web_blob.dart`;
/// keeping it in leaves is what lets the dialog itself be tested on the Dart VM.
class WebFileTransfer implements FileTransfer {
  const WebFileTransfer();

  @override
  void download(String filename, List<int> bytes) {
    final url = html.Url.createObjectUrlFromBlob(binaryBlob(bytes));
    html.AnchorElement(href: url)
      ..setAttribute('download', filename)
      ..click();
    html.Url.revokeObjectUrl(url);
  }

  @override
  Future<PickedFile?> pick({
    String accept = '.bin,*',
    Duration timeout = const Duration(seconds: 120),
  }) async {
    final upload = html.FileUploadInputElement();
    upload.accept = accept;
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
    final bytes = base64Decode(dataUrl.substring(dataUrl.indexOf(',') + 1));
    return (name: files[0].name, bytes: bytes);
  }
}

void setWebFileTransfer() => setFileTransfer(const WebFileTransfer());
