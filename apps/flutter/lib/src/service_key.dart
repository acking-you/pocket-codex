/// Reading the human-facing parts out of a relay service key.
///
/// A key is `pcx:<device>:<kind>:<name>` when self-hosting, and
/// `pcxu:<user>:<device>:<kind>:<name>` in account mode — same shape with one
/// more segment in front. Three screens each parsed this inline and disagreed:
/// one indexed `parts[1]` for the device, which is the USER in account mode, so
/// a hosted service was labelled with the account name instead of the machine.
///
/// Counting back from the end is what makes both forms work: the name is last,
/// the kind before it, the device before that, whatever precedes is a namespace.
library;

/// The device, kind and instance name of [key].
///
/// Every field is empty for a key too short to be one (a caller passing an
/// arbitrary string), so callers can fall back on the raw key rather than
/// index-crash.
({String device, String kind, String name}) parseServiceKey(String key) {
  final parts = key.split(':');
  if (parts.length < 4) return (device: '', kind: '', name: '');
  // Anchored on the KIND, which is the one segment with a fixed vocabulary
  // (`app` / `api` / `meta`), searched from the end so the leading namespace
  // doesn't shift the offsets. Everything after it is the name — a name may
  // contain colons, and splitting on the last one would truncate it.
  final kindAt = parts.lastIndexWhere(_isKind);
  if (kindAt < 1) return (device: '', kind: '', name: '');
  return (
    device: parts[kindAt - 1],
    kind: parts[kindAt],
    name: parts.sublist(kindAt + 1).join(':'),
  );
}

/// The service kinds the relay namespaces by. `meta` never appears in a
/// discovered key (it is derived from an app host), but it is a real kind and a
/// key carrying it must still parse.
bool _isKind(String part) => part == 'app' || part == 'api' || part == 'meta';

/// The device that publishes [key], or empty when [key] isn't a service key.
String serviceKeyDevice(String key) => parseServiceKey(key).device;

/// The instance name in [key], falling back to the whole key so a label is never
/// blank.
String serviceKeyName(String key) {
  final name = parseServiceKey(key).name;
  return name.isEmpty ? key : name;
}

/// `<device> · <name>`, the way every list labels a service. Falls back to the
/// raw key when it cannot be parsed, which at least stays identifiable.
String serviceKeyLabel(String key) {
  final parsed = parseServiceKey(key);
  if (parsed.device.isEmpty) return key;
  return '${parsed.device} · ${parsed.name}';
}
