// The map against the fake node: its neighbours read in one page, a
// table longer than a page read whole across pages, a node too old to
// tell them refused, and the screen: in the menu, a marker and a line
// for each, the legend, a tapped marker's details, and the blank
// background said when no street map can be had. Then Ask: each
// neighbour asked once as the map opens and again when refreshed, the
// answers drawn in the words of the neighbours' screens, and a node
// before link version 9 not asked.

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/node_link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/map_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_map/flutter_map.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// A node of link version 3, before Neighbours.
class OldNode extends FakeNode {
  OldNode({required super.now, required super.sodium, required super.app});
  @override
  Future<NodeInfo> readNode() async => NodeInfo(3, FakeNode.id, 1);
}

/// A node of link version 8, before Ask.
class Version8Node extends FakeNode {
  Version8Node({required super.now, required super.sodium, required super.app});
  @override
  Future<NodeInfo> readNode() async => NodeInfo(8, FakeNode.id, 1);
}

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 7);

  /// An app connected to the fake node, or to the node `make` gives.
  void run(
    void Function(FakeAsync, AppModel) body, {
    FakeNode Function(DateTime Function(), List<int>)? make,
  }) {
    fakeAsync((async) {
      DateTime now() => start.add(async.elapsed);
      final app = AppModel(
        sodium: sodium,
        me: Me(MemoryStore(), sodium, now),
        session: Session(store: MemoryStore(), now: now),
        contacts: Contacts(MemoryStore()),
        now: now,
      );
      app.load();
      async.flushMicrotasks();
      if (make != null) app.fakeNode = make(now, app.me.identity);
      app.connectFake();
      async.flushMicrotasks();
      body(async, app);
      app.dispose();
    });
  }

  NeighbourTable? read(FakeAsync async, AppModel app) {
    NeighbourTable? r;
    app.session.readNeighbours().then((v) => r = v);
    async.flushMicrotasks();
    return r;
  }

  String name(Neighbour n) => nodeName(n.id);

  test('the fake\'s neighbours, one of each quality, in one page', () {
    run((async, app) {
      final t = read(async, app)!;
      expect(t.floorDbm, FakeNode.floorDbm);
      expect(t.own, FakeNode.gps);
      expect(t.list.map(name), [
        for (final n in app.fakeNode!.neighbours.values) name(n),
      ]);
      expect(
        t.list.map((n) => n.quality(t.floorDbm)).toSet(),
        Quality.values.toSet(),
      );
      expect(app.session.neighbours, same(t));
    });
  });

  test('a table longer than a page is read whole', () {
    run((async, app) {
      final fake = app.fakeNode!;
      for (var slot = 30; slot < 50; slot++) {
        fake.neighbours[slot] = Neighbour(
          [0xfa, 0x4e, 0, 0, 2, slot],
          FakeNode.gps,
          rssiDdbm: -1000,
          ageS: 0,
        );
      }
      final t = read(async, app)!;
      expect(t.list.length, 25);
      final slots = fake.neighbours.keys.toList()..sort();
      expect(t.list.map(name), [
        for (final s in slots) name(fake.neighbours[s]!),
      ]);
    });
  });

  test('a node before link version 4 is not asked', () {
    run((async, app) {
      expect(app.session.node!.version, 3);
      Object? error;
      app.session.readNeighbours().catchError((Object e) {
        error = e;
        return (
          floorDbm: 0,
          own: const Position(0, 0),
          list: <Neighbour>[],
          at: start,
        );
      });
      async.flushMicrotasks();
      expect(error, isStateError);
    }, make: (now, me) => OldNode(now: now, sodium: sodium, app: me));
  });

  test('each neighbour asked once, the answers kept, the next ask anew', () {
    run((async, app) {
      final fake = app.fakeNode!;
      final s = app.session;
      final t = read(async, app)!;
      s.askNeighbours(t.list);
      async.flushMicrotasks();
      expect(fake.asked, [for (final n in t.list) hex(n.id)]);
      expect(s.answers, isEmpty);
      async.elapse(FakeNode.answerAfter);
      // Every neighbour but the one whose answer never comes.
      expect(s.answers.keys.toSet(), fake.statuses.keys.toSet());
      expect(s.answers.length, t.list.length - 1);
      final usb = s.answers[hex(t.list.first.id)]!;
      expect(usb.lines, ['FIXED RELAY', 'POWER USB', 'KNOWN 6/4/3', 'V9']);

      // Asked again, the old answers go.
      fake.statuses.remove(hex(t.list.first.id));
      s.askNeighbours(t.list);
      async.flushMicrotasks();
      expect(s.answers, isEmpty);
      async.elapse(FakeNode.answerAfter);
      expect(s.answers.length, t.list.length - 2);
      expect(fake.asked.length, 2 * t.list.length);
    });
  });

  test('an ask of anything but a node ID is refused', () {
    run((async, app) {
      Object? error;
      app.fakeNode!.ask([1, 2, 3]).catchError((Object e) => error = e);
      async.flushMicrotasks();
      expect(error, isA<AttException>());
      expect((error as AttException).code, AttError.invalidLength);
    });
  });

  test('a node before link version 9 is not asked', () {
    run((async, app) {
      expect(app.session.node!.version, 8);
      final t = read(async, app)!;
      Object? error;
      app.session.askNeighbours(t.list).catchError((Object e) => error = e);
      async.flushMicrotasks();
      expect(error, isStateError);
      expect(app.fakeNode!.asked, isEmpty);
    }, make: (now, me) => Version8Node(now: now, sodium: sodium, app: me));
  });

  /// An app connected to the fake node, or to the node `make` gives, its
  /// map open on tiles from a server that answers nothing, as with no
  /// internet and nothing kept on the phone.
  Future<AppModel> openMap(
    WidgetTester t, {
    FakeNode Function(DateTime Function(), List<int>)? make,
  }) async {
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, DateTime.now),
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
      now: DateTime.now,
    );
    await t.runAsync(app.load);
    if (make != null) app.fakeNode = make(DateTime.now, app.me.identity);
    app.connectFake();
    await t.pump();
    await t.pump();
    await t.pumpWidget(
      MaterialApp(
        home: MapScreen(
          app,
          tiles: () => NetworkTileProvider(
            cachingProvider: const DisabledMapCachingProvider(),
          ),
        ),
      ),
    );
    await t.pump();
    return app;
  }

  Future<void> shut(WidgetTester t, AppModel app) async {
    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  }

  testWidgets('the map asks as it opens and when refreshed, and says the '
      'answers in the screens\' words', (t) async {
    final print = debugPrint;
    debugPrint = (_, {wrapWidth}) {};
    final app = await openMap(t);
    final fake = app.fakeNode!;
    final list = fake.neighbours.values.toList();
    await t.pump(FakeNode.answerAfter);
    // Each asked once, though every status reads the table again.
    expect(fake.asked.length, list.length);
    expect(fake.asked.toSet().length, list.length);
    expect(find.text('FIXED RELAY'), findsNWidgets(2));
    expect(find.text('CARRIED RELAY'), findsOneWidget);
    expect(find.text('SLEEPY ANCHORED'), findsOneWidget);

    // The sleepy one's details say all its answer carries.
    Finder icon(String label) =>
        find.byWidgetPredicate((w) => w is Icon && w.semanticLabel == label);
    final sleepy = list.singleWhere((n) => n.sleepy);
    await t.tap(icon(nodeName(sleepy.id)));
    await t.pumpAndSettle();
    expect(
      find.text('Its screen: SLEEPY ANCHORED, BATTERY 57%, KNOWN 4/2/1, V9'),
      findsOneWidget,
    );
    await t.tapAt(Offset.zero);
    await t.pumpAndSettle();

    // Refreshed, each is asked again, and the answers replaced.
    fake.statuses.updateAll((_, s) => const NeighbourStatus());
    await t.tap(find.byTooltip('Read again'));
    await t.pump();
    expect(find.text('FIXED RELAY'), findsNothing);
    await t.pump(FakeNode.answerAfter);
    expect(fake.asked.length, 2 * list.length);
    expect(find.text('RELAY NO PLACE'), findsNWidgets(4));
    await shut(t, app);
    debugPrint = print;
  });

  testWidgets('the map does not ask a node before link version 9', (t) async {
    final print = debugPrint;
    debugPrint = (_, {wrapWidth}) {};
    final app = await openMap(
      t,
      make: (now, me) => Version8Node(now: now, sodium: sodium, app: me),
    );
    await t.pump(FakeNode.answerAfter);
    expect(find.textContaining('neighbours, read'), findsOneWidget);
    expect(app.fakeNode!.asked, isEmpty);
    await t.tap(find.byTooltip('Read again'));
    await t.pump(FakeNode.answerAfter);
    expect(app.fakeNode!.asked, isEmpty);
    await shut(t, app);
    debugPrint = print;
  });

  testWidgets('the map: markers, lines, legend, details, no tiles', (t) async {
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, DateTime.now),
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
      now: DateTime.now,
    );
    await t.runAsync(app.load);
    await t.pumpWidget(BarkApp(app));
    await t.tap(find.byType(PopupMenuButton<Widget Function()>));
    await t.pumpAndSettle();
    expect(find.text('Map'), findsOneWidget);
    await t.tapAt(Offset.zero);
    await t.pumpAndSettle();
    app.connectFake();
    await t.pump();
    await t.pump();
    expect(app.session.connected, isTrue);

    // Tiles from a server that answers nothing, as with no internet and
    // nothing kept on the phone; the map's line for each kept out of the
    // test's output.
    final print = debugPrint;
    debugPrint = (_, {wrapWidth}) {};
    await t.pumpWidget(
      MaterialApp(
        home: MapScreen(
          app,
          tiles: () => NetworkTileProvider(
            cachingProvider: const DisabledMapCachingProvider(),
          ),
        ),
      ),
    );
    await t.pump();
    await t.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 300)),
    );
    await t.pump();
    final fake = app.fakeNode!.neighbours.values.toList();
    expect(find.textContaining('${fake.length} neighbours'), findsOneWidget);
    expect(find.text('© OpenStreetMap contributors'), findsOneWidget);
    final lines = t.widget<PolylineLayer>(find.byType(PolylineLayer)).polylines;
    expect(lines.length, fake.length);
    expect(
      lines.map((l) => l.color).toSet(),
      Quality.values.map(qualityColor).toSet(),
    );
    Finder icon(String label) =>
        find.byWidgetPredicate((w) => w is Icon && w.semanticLabel == label);
    expect(icon('this node'), findsOneWidget);
    for (final n in fake) {
      expect(icon(name(n)), findsOneWidget);
    }
    expect(find.textContaining('No street map here'), findsOneWidget);

    // The demoted one, fragile: its details.
    final demoted = fake.singleWhere((n) => n.demoted);
    await t.tap(icon(name(demoted)));
    await t.pumpAndSettle();
    expect(find.text(name(demoted)), findsOneWidget);
    expect(
      find.textContaining('Link fragile: -115.0 dBm, 3.0 dB'),
      findsOneWidget,
    );
    expect(find.textContaining('demoted'), findsOneWidget);
    expect(find.textContaining('m away'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    debugPrint = print;
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });
}
