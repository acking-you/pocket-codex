import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/github_avatar.dart';

Widget _host(Widget child) => MaterialApp(
  home: Scaffold(body: Center(child: child)),
);

void main() {
  // The miss memo is module-level, so a failure recorded by one test would
  // change what a later one renders.
  setUp(debugResetAvatarMisses);

  // The test binding answers every image request with a 400, so a fetch here
  // always fails — which is the path that has to degrade well.
  testWidgets('an account id requests its avatar and falls back on failure', (
    t,
  ) async {
    await t.pumpWidget(
      _host(
        const GitHubAvatar(
          accountId: '73544345',
          fallbackIcon: Icons.person_outline,
        ),
      ),
    );

    final image = t.widget<Image>(find.byType(Image));
    // `cacheWidth` wraps the provider, which is the decode bound doing its job:
    // the source is a 460px PNG and the slot is 36.
    final resize = image.image as ResizeImage;
    expect(resize.width, isNotNull);
    final provider = resize.imageProvider as NetworkImage;
    expect(provider.url, contains('avatars.githubusercontent.com/u/73544345'));
    // Asks GitHub for the size actually drawn, too.
    expect(provider.url, contains('?s='));

    // The glyph holds the slot before the fetch resolves, so a slow network
    // never leaves a hole where the identity should be…
    expect(find.byIcon(Icons.person_outline), findsOneWidget);
    // …and it is still there once the fetch has failed.
    await t.pumpAndSettle();
    expect(find.byIcon(Icons.person_outline), findsOneWidget);
  });

  testWidgets('no account id renders the fallback without a request', (
    t,
  ) async {
    await t.pumpWidget(
      _host(
        const GitHubAvatar(accountId: null, fallbackIcon: Icons.dns_outlined),
      ),
    );

    // A self-hosted user has no GitHub identity, so nothing should be fetched.
    expect(find.byType(Image), findsNothing);
    expect(find.byIcon(Icons.dns_outlined), findsOneWidget);
  });

  testWidgets('a blank account id is treated as absent', (t) async {
    await t.pumpWidget(
      _host(
        const GitHubAvatar(accountId: '   ', fallbackIcon: Icons.dns_outlined),
      ),
    );

    expect(find.byType(Image), findsNothing);
    expect(find.byIcon(Icons.dns_outlined), findsOneWidget);
  });

  testWidgets('a failed id is not re-requested on the next build', (t) async {
    const avatar = GitHubAvatar(
      accountId: '999001',
      fallbackIcon: Icons.person_outline,
    );
    await t.pumpWidget(_host(avatar));
    await t.pumpAndSettle();

    // The miss is remembered, so a rebuild shows the glyph directly instead of
    // asking again — the identity row rebuilds on every services refresh.
    await t.pumpWidget(_host(const SizedBox()));
    await t.pumpWidget(_host(avatar));
    expect(find.byType(Image), findsNothing);
    expect(find.byIcon(Icons.person_outline), findsOneWidget);
  });
}
