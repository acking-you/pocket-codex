import 'dart:io';

import 'package:file_saver/file_saver.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';

/// Save an existing file with the native picker; null means cancellation.
/// Mobile copies from the path without buffering the file in a platform call.
Future<String?> exportFile(String path, String name) async {
  if (defaultTargetPlatform == TargetPlatform.android ||
      defaultTargetPlatform == TargetPlatform.iOS) {
    return FileSaver.instance.saveAs(
      name: name,
      filePath: path,
      includeExtension: false,
      mimeType: MimeType.other,
    );
  }
  final location = await getSaveLocation(suggestedName: name);
  if (location == null) return null;
  final source = File(path);
  final destination = File(location.path);
  final sameFile =
      await destination.exists() &&
      await FileSystemEntity.identical(
        await source.resolveSymbolicLinks(),
        await destination.resolveSymbolicLinks(),
      );
  if (!sameFile) await source.copy(location.path);
  return location.path;
}

/// Export bounded bytes, removing staging files on every outcome.
Future<String?> exportBytes(Uint8List bytes, String name) async {
  final dir = await Directory.systemTemp.createTemp('pcx-export-');
  try {
    final file = await File('${dir.path}/content').writeAsBytes(bytes);
    return await exportFile(file.path, name);
  } finally {
    await dir.delete(recursive: true);
  }
}
