// The map against the fake node: its neighbours read in one page, a
// table longer than a page read whole across pages, a node too old to
// tell them refused, and the screen: in the menu, a marker and a line
// for each, the legend, a tapped marker's details, and the blank
// background said when no street map can be had.

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
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
