import 'dart:html' as html;
import 'dart:typed_data';

/// A Blob holding exactly `bytes`, for handing a file to the user.
///
/// The coercion is the whole point. `html.Blob([someListOfInt])` marshals a plain Dart
/// `List<int>` to a JS `Array`, and an Array is not a `BufferSource`, so the Blob spec
/// falls through to stringifying it: the seven bytes `[0, 1, 127, 128, 255, 44, 10]`
/// become the twenty-one characters `0,1,127,128,255,44,10`. Nothing throws. The export
/// simply downloads a decimal transcript of the file instead of the file, and only
/// whoever tries to load it later finds out.
///
/// Both callers happen to pass a `Uint8List` today — `utf8.encode` and the dialog's
/// `_hexDecode` both return one — which is the only reason this has never been seen. That
/// is an accident of two call sites, not a property of the `List<int>` they are typed as,
/// so the conversion belongs here rather than in a comment asking callers to be careful.
html.Blob binaryBlob(List<int> bytes) => html.Blob(
  [bytes is Uint8List ? bytes : Uint8List.fromList(bytes)],
  'application/octet-stream',
);
