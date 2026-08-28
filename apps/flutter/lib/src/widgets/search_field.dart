import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';

/// The app's filter box: a filled, borderless field on the control radius.
///
/// Borderless because these sit inside an already-bordered toolbar or card — a
/// second outline there reads as a box in a box, which is why the design fills
/// instead of stroking. Distinct from a dialog's field, which does take the
/// default outline since it stands alone on the sheet.
class SearchField extends StatelessWidget {
  /// Creates a filter field.
  const SearchField({
    super.key,
    required this.hintText,
    required this.onChanged,
    this.controller,
    this.suffix,
  });

  /// Placeholder describing what is filtered.
  final String hintText;

  /// Called on every keystroke; callers debounce if the filter is expensive.
  final ValueChanged<String> onChanged;

  /// Optional external controller, for callers that clear or read the text.
  final TextEditingController? controller;

  /// Trailing affordance inside the field, e.g. a clear button.
  final Widget? suffix;

  /// The design's filter box: 13pt text over a 34-square glyph well, padded 9
  /// vertically so it sits a step shorter than a default Material field.
  static const double _fontSize = 13;
  static const double _iconSize = 18;
  static const double _iconWell = 34;
  static const double _verticalPadding = 9;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return TextField(
      controller: controller,
      onChanged: onChanged,
      style: const TextStyle(fontSize: _fontSize),
      decoration: InputDecoration(
        isDense: true,
        prefixIcon: const Icon(Icons.search, size: _iconSize),
        prefixIconConstraints: const BoxConstraints(
          minWidth: _iconWell,
          minHeight: _iconWell,
        ),
        suffixIcon: suffix,
        hintText: hintText,
        hintStyle: TextStyle(
          fontSize: _fontSize,
          color: scheme.onSurfaceVariant,
        ),
        filled: true,
        fillColor: scheme.surfaceContainerHighest,
        contentPadding: const EdgeInsets.symmetric(vertical: _verticalPadding),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
          borderSide: BorderSide.none,
        ),
      ),
    );
  }
}
