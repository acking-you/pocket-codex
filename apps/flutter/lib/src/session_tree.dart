/// A bounded inventory's explicit parent links, independent of recency groups.
class SessionTree<T> {
  SessionTree(Iterable<T> items, this.idOf, this.parentOf) {
    for (final item in items) {
      byId[idOf(item)] = item;
    }
    for (final item in byId.values) {
      final id = idOf(item);
      final parent = parentOf(item);
      final seen = <String>{id};
      var cursor = parent;
      var cycle = false;
      while (cursor != null && byId.containsKey(cursor)) {
        if (!seen.add(cursor)) {
          cycle = true;
          break;
        }
        cursor = parentOf(byId[cursor] as T);
      }
      if (!cycle && parent != null && byId.containsKey(parent)) {
        children.putIfAbsent(parent, () => []).add(item);
      } else {
        roots.add(item);
      }
    }
  }

  final String Function(T) idOf;
  final String? Function(T) parentOf;
  final Map<String, T> byId = {};
  final Map<String, List<T>> children = {};
  final List<T> roots = [];

  /// Keep ancestors when a descendant matches a search or status filter.
  Set<String> withAncestors(Iterable<String> ids) {
    final out = <String>{};
    for (final id in ids) {
      String? cursor = id;
      while (cursor != null && out.add(cursor)) {
        final item = byId[cursor];
        cursor = item == null ? null : parentOf(item);
      }
    }
    return out;
  }

  Iterable<({T item, int depth})> visible(
    T root,
    Set<String> expanded, {
    int depth = 0,
  }) sync* {
    yield (item: root, depth: depth);
    if (expanded.contains(idOf(root))) {
      for (final child in children[idOf(root)] ?? <T>[]) {
        yield* visible(child, expanded, depth: depth + 1);
      }
    }
  }
}
