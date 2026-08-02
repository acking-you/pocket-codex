/// Syntax highlighting for fenced code blocks.
///
/// Backed by `re_highlight`, a pure-Dart port of highlight.js (grammars synced
/// to 11.9). Chosen over the widget-shaped alternatives because it hands back a
/// [TextSpan] rather than a widget: the transcript's code block already owns its
/// frame, header and copy button, and only the text inside needs colouring.
///
/// Only the languages a coding agent actually emits are registered. The full
/// catalogue is ~200 grammars; importing them all would bundle every one of them
/// into the app for the sake of Prolog and AppleScript.
library;

import 'package:flutter/material.dart';
import 'package:re_highlight/languages/bash.dart';
import 'package:re_highlight/languages/c.dart';
import 'package:re_highlight/languages/cmake.dart';
import 'package:re_highlight/languages/cpp.dart';
import 'package:re_highlight/languages/csharp.dart';
import 'package:re_highlight/languages/css.dart';
import 'package:re_highlight/languages/dart.dart';
import 'package:re_highlight/languages/diff.dart';
import 'package:re_highlight/languages/dockerfile.dart';
import 'package:re_highlight/languages/go.dart';
import 'package:re_highlight/languages/ini.dart';
import 'package:re_highlight/languages/java.dart';
import 'package:re_highlight/languages/javascript.dart';
import 'package:re_highlight/languages/json.dart';
import 'package:re_highlight/languages/kotlin.dart';
import 'package:re_highlight/languages/lua.dart';
import 'package:re_highlight/languages/makefile.dart';
import 'package:re_highlight/languages/markdown.dart';
import 'package:re_highlight/languages/php.dart';
import 'package:re_highlight/languages/powershell.dart';
import 'package:re_highlight/languages/python.dart';
import 'package:re_highlight/languages/ruby.dart';
import 'package:re_highlight/languages/rust.dart';
import 'package:re_highlight/languages/scala.dart';
import 'package:re_highlight/languages/shell.dart';
import 'package:re_highlight/languages/sql.dart';
import 'package:re_highlight/languages/swift.dart';
import 'package:re_highlight/languages/typescript.dart';
import 'package:re_highlight/languages/xml.dart';
import 'package:re_highlight/languages/yaml.dart';
import 'package:re_highlight/re_highlight.dart';
import 'package:re_highlight/styles/atom-one-dark.dart';
import 'package:re_highlight/styles/atom-one-light.dart';

/// The registry, built once. Grammar compilation is not free, and a streaming
/// turn re-renders every code block in the transcript on every chunk.
final Highlight _hl = () {
  final h = Highlight()
    ..registerLanguages({
      'bash': langBash,
      'c': langC,
      'cmake': langCmake,
      'cpp': langCpp,
      'csharp': langCsharp,
      'css': langCss,
      'dart': langDart,
      'diff': langDiff,
      'dockerfile': langDockerfile,
      'go': langGo,
      // Also answers to `toml`, via the grammar's own alias list.
      'ini': langIni,
      'java': langJava,
      'javascript': langJavascript,
      'json': langJson,
      'kotlin': langKotlin,
      'lua': langLua,
      'makefile': langMakefile,
      'markdown': langMarkdown,
      'php': langPhp,
      'powershell': langPowershell,
      'python': langPython,
      'ruby': langRuby,
      'rust': langRust,
      'scala': langScala,
      'shell': langShell,
      'sql': langSql,
      'swift': langSwift,
      'typescript': langTypescript,
      'xml': langXml,
      'yaml': langYaml,
    });
  return h;
}();

/// Fence info strings we see in practice that aren't the grammar's own name or
/// one of its declared aliases.
const Map<String, String> _aliases = {
  'sh': 'bash',
  'zsh': 'bash',
  'shell-session': 'shell',
  'console': 'shell',
  'ps1': 'powershell',
  'pwsh': 'powershell',
  'py': 'python',
  'py3': 'python',
  'js': 'javascript',
  'jsx': 'javascript',
  'mjs': 'javascript',
  'cjs': 'javascript',
  'ts': 'typescript',
  'tsx': 'typescript',
  'rs': 'rust',
  'yml': 'yaml',
  'html': 'xml',
  'svg': 'xml',
  'md': 'markdown',
  'c++': 'cpp',
  'cc': 'cpp',
  'h': 'cpp',
  'hpp': 'cpp',
  'cs': 'csharp',
  'rb': 'ruby',
  'kt': 'kotlin',
  'golang': 'go',
  'patch': 'diff',
  'make': 'makefile',
  'dockerfile': 'dockerfile',
  'docker': 'dockerfile',
  'toml': 'ini',
  'conf': 'ini',
  'cfg': 'ini',
};

/// Longest source we will colour. Highlighting is a regex sweep over the whole
/// string on every rebuild; past this, plain monospace text is the honest
/// trade — the block is a log dump, not something anyone reads as code.
const int _maxHighlightChars = 20000;

/// [code] as a highlighted span, or a plain one when the language is unknown,
/// unsupported, or the block is too long to be worth sweeping.
///
/// [language] is the fence's info string (`” ```rust `” → `rust`); empty means
/// the fence carried none. Deliberately no auto-detection: guessing on a
/// three-line snippet mislabels far more often than it helps, and a wrong
/// grammar colours the text wrongly rather than not at all.
TextSpan highlightCode({
  required String code,
  required String language,
  required TextStyle base,
  required Brightness brightness,
  bool allowItalic = true,
}) {
  final lang = resolveLanguage(language);
  if (lang == null || code.length > _maxHighlightChars) {
    return TextSpan(text: code, style: base);
  }
  try {
    final result = _hl.highlight(code: code, language: lang);
    final renderer = TextSpanRenderer(
      base,
      brightness == Brightness.dark ? atomOneDarkTheme : atomOneLightTheme,
    );
    result.render(renderer);
    // The renderer yields null for input it produced no nodes for (an empty
    // block); fall back rather than render nothing.
    final span = renderer.span ?? TextSpan(text: code, style: base);
    // The highlight themes italicise comments; in a monospace code view with a
    // CJK fallback that slant reads as distorted, so callers can force upright.
    return allowItalic ? span : _upright(span);
  } catch (_) {
    // A grammar that throws on pathological input must never take the message
    // down with it — the text still has to be readable.
    return TextSpan(text: code, style: base);
  }
}

/// Rebuild [span] with every node's slant cleared to upright.
TextSpan _upright(TextSpan span) => TextSpan(
  text: span.text,
  style: span.style?.copyWith(fontStyle: FontStyle.normal),
  children: span.children?.map((c) => c is TextSpan ? _upright(c) : c).toList(),
);

/// The registered grammar name for a fence info string, or null when we have
/// no grammar for it. Exposed for tests.
@visibleForTesting
String? resolveLanguage(String info) {
  // A fence may carry more than the name (```dart title="x"), and casing is
  // whatever the model felt like.
  final head = info.trim().split(RegExp(r'[\s,:]')).first.toLowerCase();
  if (head.isEmpty || head == 'text' || head == 'plaintext') return null;
  final name = _aliases[head] ?? head;
  return _hl.getLanguage(name) == null ? null : name;
}
