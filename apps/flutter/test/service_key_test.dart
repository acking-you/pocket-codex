// Reading device / kind / name out of a relay service key, in both the
// self-hosted and account-mode shapes.

import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/service_key.dart';

void main() {
  test('parses a self-hosted key', () {
    final parsed = parseServiceKey('pcx:lb7666:app:default');
    expect(parsed.device, 'lb7666');
    expect(parsed.kind, 'app');
    expect(parsed.name, 'default');
  });

  test('an account key names the machine, not the account', () {
    // The regression this helper exists for: an account key carries the user
    // ahead of the device, so reading the device positionally from the front
    // labelled every hosted service with the account login.
    final parsed = parseServiceKey('pcxu:acking-you:lb7666:api:default');
    expect(parsed.device, 'lb7666');
    expect(parsed.kind, 'api');
    expect(parsed.name, 'default');
    expect(
      serviceKeyLabel('pcxu:acking-you:lb7666:api:default'),
      'lb7666 · default',
    );
  });

  test('a name may contain colons', () {
    // Nothing forbids it, and splitting on the last colon would truncate the
    // name to its final fragment.
    final parsed = parseServiceKey('pcx:box:app:my:instance');
    expect(parsed.device, 'box');
    expect(parsed.name, 'my:instance');
  });

  test('the meta kind parses too', () {
    // Never returned by discovery — it is derived from an app host — but it is a
    // real kind and a key carrying it must not fall through to "unparseable".
    expect(parseServiceKey('pcx:box:meta:default').kind, 'meta');
  });

  test('a non-key yields empties, and labels fall back to the raw string', () {
    // Callers pass whatever they hold; an index-based parse would crash here.
    expect(parseServiceKey('not-a-key').device, '');
    expect(serviceKeyLabel('not-a-key'), 'not-a-key');
    expect(serviceKeyName('not-a-key'), 'not-a-key');
    expect(serviceKeyDevice(''), '');
  });
}
