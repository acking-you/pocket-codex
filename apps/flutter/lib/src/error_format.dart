import 'package:flutter_rust_bridge/flutter_rust_bridge.dart'
    show AnyhowException;

/// Turn any error thrown across the bridge into a human-readable message.
///
/// Rust `anyhow` errors surface as [AnyhowException] whose `message` carries
/// the error chain *and* a `Stack backtrace:` dump full of `<unknown>`
/// frames. Showing that verbatim floods the UI, so we unwrap the message
/// and drop everything from the backtrace marker onward, keeping just the
/// error (and its `caused by` chain).
String friendlyError(Object error) {
  var text = error is AnyhowException ? error.message : error.toString();

  // anyhow appends the backtrace after a blank line; cut it off. Match a few
  // spellings defensively (`Stack backtrace:` / `Backtrace:`).
  for (final marker in const [
    '\nStack backtrace',
    '\nBacktrace',
    'Stack backtrace:',
  ]) {
    final idx = text.indexOf(marker);
    if (idx != -1) {
      text = text.substring(0, idx);
      break;
    }
  }

  return text.trim();
}
