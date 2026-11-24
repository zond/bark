// A newer app downloaded in the app and handed to the installer: the APK
// written whole or not at all, one kept for its version used again, a tap
// while one downloads doing nothing, a failed download tried again; and
// what the home screen's bar and Settings' App row say meanwhile.

import 'dart:async';
import 'dart:io';

import 'package:bark_app/app.dart';
import 'package:bark_app/app_update.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/releases.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'fake_ble.dart';
import 'releases_test.dart' show FakeFetch, InstallNotifier, answer, served;

/// APKs kept by version, downloaded as the test lets them: at once, or
/// when [gate] completes; a download fails while [fail] is set.
class FakeApks implements Apks {
  final Map<int, String> whole = {};
  final List<String> downloads = [];
  Completer<void>? gate;
  void Function(double?)? progress;
  Object? fail;

  @override
  Future<String?> kept(int app) async => whole[app];

  @override
  Future<String> download(
    String url,
    int app,
    void Function(double?) progress,
  ) async {
    downloads.add(url);
    this.progress = progress;
    await gate?.future;
    if (fail != null) throw fail!;
    return whole[app] = '/apk/bark-$app.apk';
  }
}

class _RealHttp extends HttpOverrides {}

const release = Release(5, app: 5, firmware: 5, apk: 'https://x/apk5');

void main() {
  group('the APK folder', () {
    late Directory dir;
    setUp(() async => dir = await Directory.systemTemp.createTemp('apk'));
    tearDown(() => dir.delete(recursive: true));

    test('written whole for its version, the one before removed', () async {
      final got = <String>[];
      final apks = ApkDir(
        dir: () async => dir,
        get: (url, to, progress) async {
          got.add(url);
          await to.writeAsBytes([1, 2, 3, 4]);
          progress(2, 4);
          progress(4, 4);
        },
      );
      expect(await apks.kept(5), isNull);
      final done = <double?>[];
      final path = await apks.download('https://x/apk5', 5, done.add);
      expect(path, '${dir.path}/apk/bark-5.apk');
      expect(done, [0.5, 1.0]);
      expect(await File(path).readAsBytes(), [1, 2, 3, 4]);
      expect(await apks.kept(5), path);
      await apks.download('https://x/apk6', 6, (_) {});
      expect(await apks.kept(5), isNull);
      expect(await apks.kept(6), '${dir.path}/apk/bark-6.apk');
      expect(got, ['https://x/apk5', 'https://x/apk6']);
    });

    test('a failed download keeps nothing', () async {
      final apks = ApkDir(
        dir: () async => dir,
        get: (url, to, progress) async {
          await to.writeAsBytes([1, 2]);
          progress(2, null);
          throw const HttpException('cut off');
        },
      );
      final done = <double?>[];
      await expectLater(
        apks.download('https://x/apk5', 5, done.add),
        throwsA(isA<HttpException>()),
      );
      expect(done, [null]);
      expect(await apks.kept(5), isNull);
      expect(await Directory('${dir.path}/apk').list().toList(), isEmpty);
    });

    // Widget tests answer every request 400 unless told otherwise.
    test('over HTTP: the body and its length, or the status', () {
      return HttpOverrides.runWithHttpOverrides(
        () => _overHttp(dir),
        _RealHttp(),
      );
    });
  });

  group('the model', () {
    test('downloaded once, installed at each tap', () async {
      final apks = FakeApks()..gate = Completer();
      final installed = <String>[];
      final u = AppUpdate(apks, (p) async => installed.add(p));
      expect(u.state(5), AppGet.idle);
      final first = u.get(release);
      await pumpEventQueue();
      expect(u.state(5), AppGet.downloading);
      expect(u.percent, isNull);
      apks.progress!(0.421);
      expect(u.percent, 42);
      // A second tap starts no second download.
      await u.get(release);
      expect(apks.downloads, ['https://x/apk5']);
      apks.gate!.complete();
      await first;
      expect(u.state(5), AppGet.ready);
      expect(installed, ['/apk/bark-5.apk']);
      // The installer was left: the kept APK, at once.
      await u.get(release);
      expect(apks.downloads, hasLength(1));
      expect(installed, ['/apk/bark-5.apk', '/apk/bark-5.apk']);
      // Another version is not this one's.
      expect(u.state(6), AppGet.idle);
    });

    test('a failed download, said, and tried again', () async {
      final apks = FakeApks()..fail = const HttpException('no network');
      final installed = <String>[];
      final lines = <String>[];
      final u = AppUpdate(apks, (p) async => installed.add(p))
        ..noted = lines.add;
      await u.get(release);
      expect(u.state(5), AppGet.failed);
      expect(installed, isEmpty);
      expect(lines.single, startsWith('Downloading app 5 failed'));
      apks.fail = null;
      await u.get(release);
      expect(u.state(5), AppGet.ready);
      expect(installed, ['/apk/bark-5.apk']);
    });

    test('kept from an earlier run: no download', () async {
      final apks = FakeApks()..whole[5] = '/apk/bark-5.apk';
      final installed = <String>[];
      final u = AppUpdate(apks, (p) async => installed.add(p));
      final states = <AppGet>[];
      u.addListener(() => states.add(u.state(5)));
      await u.get(release);
      expect(apks.downloads, isEmpty);
      expect(states, [AppGet.ready]);
      expect(installed, ['/apk/bark-5.apk']);
    });
  });

  group('what the rows say', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());

    Future<(AppModel, FakeApks, InstallNotifier)> model(
      WidgetTester t, {
      int app = 3,
    }) async {
      final apks = FakeApks()..gate = Completer();
      final n = InstallNotifier();
      final a = AppModel(
        sodium: sodium,
        me: Me(MemoryStore(), sodium, DateTime.now),
        session: Session(store: MemoryStore(), now: DateTime.now),
        contacts: Contacts(MemoryStore()),
        now: DateTime.now,
        notifier: n,
        apks: apks,
        releases: Releases(
          MemoryStore(),
          now: DateTime.now,
          fetch: FakeFetch(answer(5)).call,
          download: served,
          app: app,
        ),
      );
      await t.runAsync(a.load);
      await t.runAsync(a.releases.check);
      return (a, apks, n);
    }

    testWidgets('the home screen\'s bar', (t) async {
      final (app, apks, n) = await model(t);
      await t.pumpWidget(BarkApp(app));
      expect(find.text('Tap to install.'), findsOneWidget);
      await t.tap(find.text('App version 5 is out'));
      await t.pump();
      expect(find.text('Downloading…'), findsOneWidget);
      apks.progress!(0.5);
      await t.pump();
      expect(find.text('Downloading… 50%'), findsOneWidget);
      // No second download from a tap meanwhile.
      await t.tap(find.text('App version 5 is out'));
      await t.pump();
      expect(apks.downloads, hasLength(1));
      apks.fail = const HttpException('cut off');
      apks.gate!.complete();
      await t.pump();
      expect(find.text('Download failed. Tap to try again.'), findsOneWidget);
      expect(n.installed, isEmpty);
      apks
        ..fail = null
        ..gate = null;
      await t.tap(find.text('App version 5 is out'));
      await t.pump();
      expect(find.text('Downloaded. Tap to install.'), findsOneWidget);
      expect(n.installed, ['/apk/bark-5.apk']);
      app.disposeWithNode();
    });

    testWidgets('Settings\' App row', (t) async {
      final (app, apks, n) = await model(t);
      await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
      Future<void> press(String label) async {
        final button = find.widgetWithText(FilledButton, label);
        await t.ensureVisible(button);
        await t.tap(button);
        await t.pump();
      }

      expect(find.text('Version 3; 5 is out'), findsOneWidget);
      await press('Install');
      expect(find.text('Version 3; downloading 5…'), findsOneWidget);
      // No button while it downloads.
      expect(find.widgetWithText(FilledButton, 'Install'), findsNothing);
      apks.progress!(0.07);
      await t.pump();
      expect(find.text('Version 3; downloading 5… 7%'), findsOneWidget);
      apks.fail = const HttpException('cut off');
      apks.gate!.complete();
      await t.pump();
      expect(find.text('Version 3; 5 did not download'), findsOneWidget);
      apks
        ..fail = null
        ..gate = null;
      await press('Try again');
      expect(find.text('Version 3; 5 is downloaded'), findsOneWidget);
      expect(find.widgetWithText(FilledButton, 'Install'), findsOneWidget);
      expect(n.installed, ['/apk/bark-5.apk']);
      app.disposeWithNode();
    });

    testWidgets('a development build offers none', (t) async {
      final (app, _, _) = await model(t, app: 0);
      await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
      expect(find.text('A development build'), findsOneWidget);
      expect(find.widgetWithText(FilledButton, 'Install'), findsNothing);
      app.disposeWithNode();
    });
  });
}

Future<void> _overHttp(Directory dir) async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((rq) {
    if (rq.uri.path == '/apk') {
      rq.response
        ..contentLength = 3
        ..add([7, 8, 9]);
    } else {
      rq.response.statusCode = HttpStatus.notFound;
    }
    rq.response.close();
  });
  final url = 'http://${server.address.host}:${server.port}';
  final to = File('${dir.path}/got');
  final seen = <(int, int?)>[];
  await httpGetTo('$url/apk', to, (got, total) => seen.add((got, total)));
  expect(await to.readAsBytes(), [7, 8, 9]);
  expect(seen.last, (3, 3));
  await expectLater(
    httpGetTo('$url/none', to, (_, _) {}),
    throwsA(isA<HttpException>()),
  );
  await server.close();
}
