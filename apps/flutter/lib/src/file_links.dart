import 'dart:io';

import 'package:path/path.dart' as p;

/// A filesystem link, resolved using the host's path convention.
class FileLink {
  const FileLink({required this.href, required this.path, this.line});
  final String href;
  final String path;
  final int? line;

  String get name => path.split(RegExp(r'[/\\]')).last;

  static FileLink? parse(String href, {String? cwd}) {
    var value = href.trim();
    if (value.isEmpty || value.startsWith('#') || value.startsWith('//')) {
      return null;
    }
    final windows = RegExp(r'^[a-zA-Z]:[/\\]').hasMatch(value);
    final uri = Uri.tryParse(value);
    if (!windows && uri != null && uri.hasScheme && uri.scheme != 'file') {
      return null;
    }
    int? line;
    final fragment = value.indexOf('#');
    if (fragment >= 0) {
      final match = RegExp(
        r'^L?(\d+)',
      ).firstMatch(value.substring(fragment + 1));
      line = int.tryParse(match?.group(1) ?? '');
      value = value.substring(0, fragment);
    }
    final location = RegExp(r':(\d+)(?::\d+)?$').firstMatch(value);
    if (location != null) {
      line ??= int.tryParse(location.group(1)!);
      value = value.substring(0, location.start);
    }
    try {
      if (value.startsWith('file:')) {
        final file = Uri.parse(value);
        if (file.host.isNotEmpty && file.host != 'localhost') return null;
        value = file
            .replace(host: '')
            .toFilePath(windows: RegExp(r'^/[a-zA-Z]:/').hasMatch(file.path));
      } else {
        value = Uri.decodeComponent(value);
      }
    } on FormatException {
      return null;
    } on ArgumentError {
      return null;
    } on UnsupportedError {
      return null;
    }
    if (value.contains('\u0000')) return null;
    final isWindows =
        RegExp(r'^[a-zA-Z]:[/\\]').hasMatch(value) ||
        (cwd != null && RegExp(r'^[a-zA-Z]:[/\\]').hasMatch(cwd));
    final paths = p.Context(style: isWindows ? p.Style.windows : p.Style.posix);
    if (!paths.isAbsolute(value)) {
      if (cwd == null || cwd.isEmpty) return null;
      value = paths.join(cwd, value);
    }
    return FileLink(href: href, path: paths.normalize(value), line: line);
  }
}

/// Loopback addresses in a transcript refer to the host, not the controller.
bool isHostLocalWebUrl(Uri uri) {
  if (uri.scheme != 'http' && uri.scheme != 'https') return false;
  final host = uri.host
      .toLowerCase()
      .replaceAll(RegExp(r'^\[|\]$'), '')
      .replaceFirst(RegExp(r'\.$'), '');
  if (host == 'localhost' || host.endsWith('.localhost')) return true;
  final address = InternetAddress.tryParse(host);
  if (address == null) return false;
  final bytes = address.rawAddress;
  if (bytes.length == 4) {
    return bytes.first == 127 || bytes.every((b) => b == 0);
  }
  if (bytes.take(15).every((b) => b == 0) && bytes.last <= 1) return true;
  return bytes.take(10).every((b) => b == 0) &&
      bytes[10] == 255 &&
      bytes[11] == 255 &&
      (bytes[12] == 127 || bytes.skip(12).every((b) => b == 0));
}
