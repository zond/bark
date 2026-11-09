// The check for a newer release: the API's answer read, the app's and the
// node's versions compared, a development build and a bench node never
// asked to update, dismissals kept, a fetch at start and a day after, a
// failed fetch silent, the last answer shown at once at the next start;
// and the two bars on the home screen against the fake node.

import 'package:bark_app/alerts.dart';
import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/releases.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'update_test.dart' show imageFile;

/// The API's answer for release `v<n>` with its APK and image.
Map<String, dynamic> answer(int n, {bool apk = true, bool image = true}) => {
  'tag_name': 'v$n',
  'assets': [
    if (apk) {'name': 'bark-$n.apk', 'browser_download_url': 'https://x/apk$n'},
    {'name': 'bark-fw-$n.uf2', 'browser_download_url': 'https://x/uf2'},
    if (image)
      {'name': 'bark-fw-$n.img', 'browser_download_url': 'https://x/img$n'},
  ],
};

/// A fetch that answers `release`, or fails when it is null, and counts.
class FakeFetch {
  Map<String, dynamic>? release;
  int calls = 0;
  FakeFetch([this.release]);
  Future<Map<String, dynamic>> call() async {
    calls++;
    final r = release;
    if (r == null) throw const SocketException('no network');
    return r;
  }
}

class SocketException implements Exception {
  final String message;
  const SocketException(this.message);
  @override
  String toString() => message;
}

/// Writes down the links the app opens.
class LinkNotifier extends NoNotifier {
  final List<String> opened = [];
  LinkNotifier();
  @override
  Future<void> openLink(String url) async => opened.add(url);
}

NodeInfo node(int image, {int last = 1}) =>
    NodeInfo(updateVersion, [0xfa, 0x4e, 0, 0, 0, last], image);

void main() {
  test('a release from the API', () {
    final r = Release.parse(answer(7))!;
    expect(r.n, 7);
    expect(r.apk, 'https://x/apk7');
    expect(r.image, 'https://x/img7');
    expect(r.imageName, 'bark-fw-7.img');
    final bare = Release.parse(answer(7, apk: false, image: false))!;
    expect(bare.apk, isNull);
    expect(bare.image, isNull);
    // Files named for another N are not this release's.
    final other = Release.parse({...answer(7), 'tag_name': 'v8'})!;
    expect(other.apk, isNull);
    expect(other.image, isNull);
    for (final t in ['7', 'v0', 'v07', 'v1.2', null]) {
      expect(Release.parse({...answer(7), 'tag_name': t}), isNull);
    }
  });

  final start = DateTime.utc(2026, 11, 7);

  /// Releases of app version `app` against `fetch`, loaded from `store`.
  Releases releases(
    FakeAsync async,
    FakeFetch fetch, {
    int app = 3,
    MemoryStore? store,
  }) {
    final r = Releases(
      store ?? MemoryStore(),
      now: () => start.add(async.elapsed),
      fetch: fetch.call,
      app: app,
    );
    r.load();
    async.flushMicrotasks();
    return r;
  }

  void check(FakeAsync async, Releases r) {
    r.check();
    async.flushMicrotasks();
  }

  test('a newer app, and a newer image than the node runs', () {
    fakeAsync((async) {
      final r = releases(async, FakeFetch(answer(5)));
      expect(r.newerApp, isNull, reason: 'nothing fetched yet');
      check(async, r);
      expect(r.newerApp?.n, 5);
      expect(r.newerImage(node(4))?.n, 5);
      expect(r.newerImage(node(5)), isNull);
      expect(r.newerImage(node(6)), isNull);
      expect(r.newerImage(null), isNull);
      // A bench build, flashed by USB, takes any image and is updated by
      // USB: no word.
      expect(r.newerImage(node(0)), isNull);
    });
    fakeAsync((async) {
      final same = releases(async, FakeFetch(answer(3)));
      check(async, same);
      expect(same.newerApp, isNull);
      // A development build never asks.
      final dev = releases(async, FakeFetch(answer(5)), app: 0);
      check(async, dev);
      expect(dev.newerApp, isNull);
      expect(dev.newerImage(node(4))?.n, 5);
      // A release without the file asks for nothing.
      final bare = releases(
        async,
        FakeFetch(answer(5, apk: false, image: false)),
      );
      check(async, bare);
      expect(bare.newerApp, isNull);
      expect(bare.newerImage(node(4)), isNull);
    });
  });

  test('dismissed for a version, kept, and asked again for the next', () {
    fakeAsync((async) {
      final store = MemoryStore();
      final fetch = FakeFetch(answer(5));
      final r = releases(async, fetch, store: store);
      check(async, r);
      r.dismissApp(5);
      r.dismissImage(node(4), 5);
      async.flushMicrotasks();
      expect(r.newerApp, isNull);
      expect(r.newerImage(node(4)), isNull);
      // Another node is asked still.
      expect(r.newerImage(node(4, last: 2))?.n, 5);

      final again = releases(async, fetch, store: store);
      expect(again.newerApp, isNull);
      expect(again.newerImage(node(4)), isNull);
      fetch.release = answer(6);
      check(async, again);
      expect(again.newerApp?.n, 6);
      expect(again.newerImage(node(4))?.n, 6);
    });
  });

  test('fetched at start and a day after, not between', () {
    fakeAsync((async) {
      final fetch = FakeFetch(answer(5));
      final r = releases(async, fetch);
      check(async, r);
      expect(fetch.calls, 1);
      expect(r.checked, start);
      async.elapse(const Duration(hours: 23, minutes: 59));
      check(async, r);
      expect(fetch.calls, 1);
      async.elapse(const Duration(minutes: 1));
      check(async, r);
      expect(fetch.calls, 2);
    });
  });

  test('no network: no word, the last answer kept, tried at next start', () {
    fakeAsync((async) {
      final store = MemoryStore();
      final fetch = FakeFetch(answer(5));
      check(async, releases(async, fetch, store: store));

      fetch.release = null;
      final lines = <String>[];
      final next = releases(async, fetch, store: store)..noted = lines.add;
      // Shown at once from what the last run fetched.
      expect(next.newerApp?.n, 5);
      check(async, next);
      expect(fetch.calls, 2);
      expect(next.newerApp?.n, 5);
      expect(next.checked, start);
      expect(lines, ['Checking for a new release failed: no network']);
      // Not again in this run until a day has passed.
      check(async, next);
      expect(fetch.calls, 2);

      final none = releases(async, FakeFetch());
      check(async, none);
      expect(none.newerApp, isNull);
      expect(none.newerImage(node(4)), isNull);

      // The next start tries at once.
      check(async, releases(async, fetch, store: store));
      expect(fetch.calls, 3);
    });
  });

  group('the bars on the home screen', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());

    Future<(AppModel, LinkNotifier)> model(WidgetTester t, int app) async {
      final n = LinkNotifier();
      final a = AppModel(
        sodium: sodium,
        me: Me(MemoryStore(), sodium, DateTime.now),
        session: Session(store: MemoryStore(), now: DateTime.now),
        contacts: Contacts(MemoryStore()),
        now: DateTime.now,
        notifier: n,
        releases: Releases(
          MemoryStore(),
          now: DateTime.now,
          fetch: FakeFetch(answer(5)).call,
          download: (url) async {
            expect(url, 'https://x/img5');
            return imageFile(5, 20001);
          },
          app: app,
        ),
      );
      await t.runAsync(a.load);
      await t.runAsync(a.releases.check);
      return (a, n);
    }

    const appBar = 'A newer app, version 5, is available';
    const nodeBar = 'Your node runs version 1, version 5 is available';

    testWidgets('a newer app: opened in the browser, or dismissed', (t) async {
      final (app, n) = await model(t, 3);
      await t.pumpWidget(BarkApp(app));
      expect(find.text(appBar), findsOneWidget);
      expect(find.text(nodeBar), findsNothing);
      await t.tap(find.text(appBar));
      await t.pump();
      expect(n.opened, ['https://x/apk5']);
      await t.tap(find.byTooltip('Not for this version'));
      await t.pump();
      expect(find.text(appBar), findsNothing);
      expect(app.releases.dismissedApp, 5);
      app.dispose();
    });

    testWidgets('a node behind: the update screen with the image chosen, '
        'or dismissed for that node', (t) async {
      final (app, _) = await model(t, 0);
      await t.pumpWidget(BarkApp(app));
      expect(find.text(appBar), findsNothing, reason: 'a development build');
      expect(find.text(nodeBar), findsNothing, reason: 'no node');
      app.setFake(true);
      app.connectFake();
      await t.pump();
      expect(find.text(nodeBar), findsOneWidget);
      await t.tap(find.text(nodeBar));
      await t.pumpAndSettle();
      expect(find.text('Update firmware'), findsOneWidget);
      expect(app.updater.image?.version, 5);
      expect(find.text('Version 5'), findsOneWidget);
      await t.pageBack();
      await t.pumpAndSettle();
      await t.tap(find.byTooltip('Not for this version'));
      await t.pump();
      expect(find.text(nodeBar), findsNothing);
      expect(app.releases.dismissedNodes, {nodeName(app.session.node!.id): 5});
      await t.pumpWidget(const SizedBox());
      await app.session.disconnect();
      await t.pump();
      app.dispose();
    });
  });

  group('Settings\' Check for updates', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());

    /// The app at release `app` with the fake node of image `image`, or
    /// none, against a release list that answers release 5, or none.
    Future<AppModel> settings(
      WidgetTester t, {
      required int app,
      int? image,
      bool online = true,
    }) async {
      final a = AppModel(
        sodium: sodium,
        me: Me(MemoryStore(), sodium, DateTime.now),
        session: Session(store: MemoryStore(), now: DateTime.now),
        contacts: Contacts(MemoryStore()),
        now: DateTime.now,
        notifier: LinkNotifier(),
        releases: Releases(
          MemoryStore(),
          now: DateTime.now,
          fetch: FakeFetch(online ? answer(5) : null).call,
          app: app,
        ),
      );
      await t.runAsync(a.load);
      if (image != null) {
        a.setFake(true);
        a.fakeNode = FakeNode(
          now: DateTime.now,
          sodium: sodium,
          app: a.me.identity,
        )..image = image;
        a.connectFake();
        await t.pump();
      }
      await t.pumpWidget(MaterialApp(home: SettingsScreen(a)));
      await t.pump();
      final check = find.text('Check for updates');
      await t.ensureVisible(check);
      await t.tap(check);
      await t.runAsync(() => Future<void>.delayed(Duration.zero));
      await t.pump();
      return a;
    }

    Future<void> shut(WidgetTester t, AppModel a) async {
      await t.pumpWidget(const SizedBox());
      await a.session.disconnect();
      await t.pump();
      a.dispose();
    }

    testWidgets('up to date', (t) async {
      final a = await settings(t, app: 5, image: 5);
      expect(
        find.text('App up to date (5); node firmware up to date (5)'),
        findsOneWidget,
      );
      expect(find.text('Send to the node'), findsNothing);
      await shut(t, a);
    });

    testWidgets('a newer app, and no node linked', (t) async {
      final a = await settings(t, app: 3);
      expect(
        find.text(
          'A newer app is available, release 5: get it under App below; '
          'no node linked',
        ),
        findsOneWidget,
      );
      expect(
        find.textContaining('version 5 is available: tap'),
        findsOneWidget,
      );
      await shut(t, a);
    });

    testWidgets('a newer node firmware, sent from Node firmware', (t) async {
      final a = await settings(t, app: 5, image: 1);
      expect(
        find.text(
          'App up to date (5); node firmware 1; release 5 is newer: send '
          'it under Node firmware below',
        ),
        findsOneWidget,
      );
      expect(find.text('Send to the node'), findsOneWidget);
      expect(find.text('Pick a firmware file (bench)'), findsOneWidget);
      await shut(t, a);
    });

    testWidgets('offline', (t) async {
      final a = await settings(t, app: 5, image: 5, online: false);
      expect(find.text('Could not reach the release list'), findsOneWidget);
      await shut(t, a);
    });
  });
}
