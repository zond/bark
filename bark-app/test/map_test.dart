// The map against the fake node: its neighbours read in one page, a
// table longer than a page read whole across pages, and the screen: in
// the menu, a marker and a line for each, the legend, a tapped marker's
// details, and the blank background said when no street map can be had.
// Then Ask: each neighbour asked as the map opens, one that does not
// answer asked again every 2 s up to three asks, a refresh asking only
// those without an answer in the last 10 s, and the answers drawn in the
// words of the neighbours' screens.

import 'package:bark_app/app.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';

import 'fake_ble.dart';
import 'fake_node.dart';

import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/node_link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/fact_help.dart';
import 'package:bark_app/ui/map_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_map/flutter_map.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

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
      app.disposeWithNode();
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

  test('each neighbour asked, the silent one again every 2 s, three times '
      'at most', () {
    run((async, app) {
      final fake = app.fakeNode!..loseEvery = 0;
      final s = app.session;
      final t = read(async, app)!;
      final silent = t.list.singleWhere(
        (n) => !fake.statuses.containsKey(hex(n.id)),
      );
      s.askNeighbours(t.list);
      async.flushMicrotasks();
      expect(fake.asked, [for (final n in t.list) hex(n.id)]);
      expect(s.answers, isEmpty);
      async.elapse(FakeNode.answerAfter);
      // Every neighbour but the one whose answer never comes.
      expect(s.answers.keys.toSet(), fake.statuses.keys.toSet());
      expect(s.answers.length, t.list.length - 1);
      final usb = s.answers[hex(t.list.first.id)]!;
      expect(usb.lines, [
        'FIXED RELAY',
        'POWER USB',
        'KNOWN 6/4/3',
        'V9',
        'GET V10 14/31',
      ]);
      // Not asked again before its round's wait is over, then asked
      // again at each, and no more after three asks.
      async.elapse(Session.answerWait - FakeNode.answerAfter * 2);
      expect(fake.asked.length, t.list.length);
      async.elapse(FakeNode.answerAfter);
      expect(fake.asked.length, t.list.length + 1);
      expect(fake.asked.last, hex(silent.id));
      async.elapse(Session.answerWait);
      expect(fake.asked.length, t.list.length + 2);
      async.elapse(Session.answerWait * 5);
      expect(fake.asked.length, t.list.length + 2);
      expect(fake.asked.where((k) => k == hex(silent.id)).length, 3);
    });
  });

  test('a lost ask is asked again and answered', () {
    run((async, app) {
      final fake = app.fakeNode!..loseEvery = 2;
      final s = app.session;
      final t = read(async, app)!;
      s.askNeighbours(t.list);
      async.flushMicrotasks();
      async.elapse(FakeNode.answerAfter);
      // Of the four that answer, the second and fourth asks are lost.
      expect(s.answers.length, 2);
      async.elapse(Session.answerWait * 3);
      expect(s.answers.keys.toSet(), fake.statuses.keys.toSet());
      // Five, then the two lost and the silent one, then the one lost
      // again and the silent one.
      expect(fake.asked.length, 5 + 3 + 2);
    });
  });

  test('a refresh asks only those without an answer in the last 10 s', () {
    run((async, app) {
      final fake = app.fakeNode!..loseEvery = 0;
      final s = app.session;
      final t = read(async, app)!;
      final silent = hex(
        t.list.singleWhere((n) => !fake.statuses.containsKey(hex(n.id))).id,
      );
      s.askNeighbours(t.list);
      async.elapse(Session.answerWait * 3);
      final first = fake.asked.length;
      fake.asked.clear();
      fake.statuses.remove(hex(t.list.first.id));
      s.askNeighbours(t.list);
      async.flushMicrotasks();
      // The answers kept, the silent one asked anew.
      expect(fake.asked, [silent]);
      expect(s.answers.length, t.list.length - 1);
      async.elapse(Session.answerWait * 3);
      fake.asked.clear();
      // Past 10 s since they answered, every one is asked, and the old
      // answers go.
      async.elapse(Session.askFresh);
      s.askNeighbours(t.list);
      async.flushMicrotasks();
      expect(fake.asked, [for (final n in t.list) hex(n.id)]);
      expect(s.answers, isEmpty);
      async.elapse(Session.answerWait * 3);
      expect(s.answers.length, t.list.length - 2);
      expect(first, t.list.length + 2);
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

  /// An app connected to the fake node, or to the node `make` gives, its
  /// map open on tiles from a server that answers nothing, as with no
  /// internet and nothing kept on the phone.
  Future<AppModel> openMap(
    WidgetTester t, {
    FakeNode Function(DateTime Function(), List<int>)? make,
    int loseEvery = 0,
  }) async {
    // The test's clock, which its timers follow.
    DateTime now() => t.binding.clock.now();
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, now),
      session: Session(store: MemoryStore(), now: now),
      contacts: Contacts(MemoryStore()),
      now: now,
    );
    await t.runAsync(app.load);
    if (make != null) app.fakeNode = make(now, app.me.identity);
    app.connectFake();
    app.fakeNode!.loseEvery = loseEvery;
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
    app.disposeWithNode();
  }

  testWidgets('the map asks as it opens, again while one is silent, and '
      'when refreshed, and says the answers in the screens\' words', (t) async {
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

    // The sleepy one's details: whose node it is, a contact's home, over
    // its ID, then a row a fact in plain words, an icon named for screen
    // readers, a label and the value, in a fixed order; none of its
    // screen's words.
    Finder icon(String label) =>
        find.byWidgetPredicate((w) => w is Icon && w.semanticLabel == label);
    final sleepy = list.singleWhere((n) => n.sleepy);
    await t.runAsync(
      () => app.contacts.put(
        Contact(
          PersonCard(
            List.filled(32, 7),
            Address(sleepy.position, sleepy.id),
            0,
            'Olena',
          ),
          '',
        ),
      ),
    );
    await t.tap(icon(nodeLabel(sleepy.id)));
    await t.pumpAndSettle();
    final sheet = find.byType(BottomSheet);
    Finder line(String text) =>
        find.descendant(of: sheet, matching: find.text(text));
    Finder named(String label) =>
        find.descendant(of: sheet, matching: icon(label));
    expect(
      t.getTopLeft(line('Olena')).dy,
      lessThan(t.getTopLeft(line(nodeLabel(sleepy.id))).dy),
    );
    expect(line('SLEEPY ANCHORED'), findsNothing);
    final rows = [
      ('Role', named('role, sleepy')),
      ('Moving', named('moving')),
      ('Anchor', named('anchor')),
      ('Place', named('place, set by a person')),
      ('Power', named('battery')),
      ('Link', named('link strength, good')),
      ('Last heard', named('last heard')),
      ('Distance', named('distance')),
      ('Known', named('neighbours known')),
      ('Version', named('firmware version')),
    ];
    final ys = <double>[];
    for (final (label, row) in rows) {
      expect(row, findsOneWidget);
      // Its plain label beside its icon, on the same line.
      expect(line(label), findsOneWidget);
      expect(t.getCenter(line(label)).dy, closeTo(t.getCenter(row).dy, 1));
      ys.add(t.getTopLeft(row).dy);
    }
    for (var i = 1; i < ys.length; i++) {
      expect(ys[i], greaterThan(ys[i - 1]));
    }
    expect(
      line('sleepy: wakes in windows, speaks through its anchor'),
      findsOneWidget,
    );
    expect(line('stays put'), findsOneWidget);
    expect(named('moving').evaluate().single.widget, isA<Icon>());
    expect(
      (named('moving').evaluate().single.widget as Icon).icon,
      Icons.house,
    );
    expect(line('anchored'), findsOneWidget);
    expect(
      find.descendant(
        of: sheet,
        matching: find.textContaining('set by a person\n'),
      ),
      findsOneWidget,
    );
    expect(line('57%'), findsOneWidget);
    expect(line('4 heard \u00b7 2 strong \u00b7 1 exchanged'), findsOneWidget);
    expect(line('V9'), findsOneWidget);
    expect(line('40 s ago'), findsOneWidget);
    expect(named('exchanged'), findsNothing);
    await closeSheet(t);
    await t.pumpAndSettle();

    // One whose answer says carried, though its heartbeats did not.
    final carried = list.singleWhere(
      (n) => fake.statuses[hex(n.id)]?.carried ?? false,
    );
    await t.tap(icon(nodeLabel(carried.id)));
    await t.pumpAndSettle();
    expect(line('carried'), findsOneWidget);
    expect(
      (named('moving').evaluate().single.widget as Icon).icon,
      Icons.directions_walk,
    );
    await closeSheet(t);
    await t.pumpAndSettle();

    // The one that never answers keeps the app's flags.
    final silent = list.singleWhere(
      (n) => !fake.statuses.containsKey(hex(n.id)),
    );
    await t.tap(icon(nodeLabel(silent.id)));
    await t.pumpAndSettle();
    expect(
      find.descendant(
        of: find.byType(BottomSheet),
        matching: find.text('for now: it refused or failed a hand-over'),
      ),
      findsOneWidget,
    );
    // A relay: no Anchor row, and no contact's name.
    expect(named('role, relay'), findsOneWidget);
    expect(line("relay: passes on others' texts"), findsOneWidget);
    expect(named('anchor'), findsNothing);
    expect(line('Olena'), findsNothing);
    // Its rows from its heartbeats; none its answer would give.
    expect(
      find.descendant(
        of: find.byType(BottomSheet),
        matching: icon('neighbours known'),
      ),
      findsNothing,
    );
    await closeSheet(t);
    await t.pumpAndSettle();

    // The silent one asked twice more, then, refreshed once the answers
    // are 10 s old, each is asked again, and the answers replaced.
    await t.pump(Session.askFresh);
    expect(fake.asked.length, list.length + 2);
    fake.statuses.updateAll((_, s) => const NeighbourStatus());
    await t.tap(find.byTooltip('Read again'));
    await t.pump();
    expect(find.text('FIXED RELAY'), findsNothing);
    await t.pump(FakeNode.answerAfter);
    expect(fake.asked.length, 2 * list.length + 2);
    expect(find.text('RELAY NO PLACE'), findsNWidgets(4));
    await shut(t, app);
    debugPrint = print;
  });

  test('every fact has its help, two to four sentences', () {
    for (final MapEntry(:key, :value) in factHelp.entries) {
      final sentences = RegExp(r'[.!?](\s|$)').allMatches(value).length;
      expect(sentences, inInclusiveRange(2, 4), reason: key);
    }
  });

  testWidgets('every row of a sheet opens its help', (t) async {
    final print = debugPrint;
    debugPrint = (_, {wrapWidth}) {};
    // A tall screen, so that no marker is under the legend.
    t.view.physicalSize = const Size(1200, 2400);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final app = await openMap(t);
    final fake = app.fakeNode!;
    await t.pump(FakeNode.answerAfter);
    final seen = <String>{};
    Future<void> each(Finder marker) async {
      await t.tap(marker);
      await t.pumpAndSettle();
      final sheet = find.byType(BottomSheet);
      final rows = find.descendant(of: sheet, matching: find.byType(InkWell));
      for (var i = 0; i < rows.evaluate().length; i++) {
        final row = rows.at(i);
        final label = factHelp.keys.singleWhere(
          (l) => find
              .descendant(of: row, matching: find.text(l))
              .evaluate()
              .isNotEmpty,
        );
        await t.ensureVisible(row);
        await t.tap(row);
        await t.pumpAndSettle();
        expect(find.byType(BottomSheet), findsNWidgets(2));
        expect(find.text(factHelp[label]!), findsOneWidget);
        seen.add(label);
        Navigator.pop(t.element(find.byType(BottomSheet).last));
        await t.pumpAndSettle();
      }
      await closeSheet(t);
    }

    Finder icon(String label) =>
        find.byWidgetPredicate((w) => w is Icon && w.semanticLabel == label);
    for (final n in fake.neighbours.values) {
      await each(icon(nodeLabel(n.id)));
    }
    await each(icon('this node'));
    // All but the anchor's own note, as the fake's node is a relay.
    expect(seen, containsAll(factHelp.keys.toSet()..remove('Anchor')));
    expect(seen, contains('Anchor'));
    await shut(t, app);
    debugPrint = print;
  });

  testWidgets('a sleepy node\'s own sheet names its anchor', (t) async {
    final print = debugPrint;
    debugPrint = (_, {wrapWidth}) {};
    final app = await openMap(t);
    final fake = app.fakeNode!;
    final a = fake.neighbours[0]!;
    fake.neighbours[0] = Neighbour(
      a.id,
      a.position,
      rssiDdbm: a.rssiDdbm,
      ageS: a.ageS,
      anchor: true,
    );
    fake.sleepy = true;
    fake.changed();
    await t.pump(FakeNode.answerAfter);
    await t.tap(
      find.byWidgetPredicate(
        (w) => w is Icon && w.semanticLabel == 'this node',
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('anchored to ${nodeLabel(a.id)}'), findsOneWidget);
    expect(
      find.text('sleepy: wakes in windows, speaks through its anchor'),
      findsOneWidget,
    );
    await closeSheet(t);
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
    expect(find.text('Nearby nodes'), findsOneWidget);
    expect(find.text('Debug'), findsNothing);
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
      expect(icon(nodeLabel(n.id)), findsOneWidget);
    }
    expect(find.textContaining('No street map here'), findsOneWidget);

    // The demoted one, fragile: its details.
    final demoted = fake.singleWhere((n) => n.demoted);
    await t.tap(icon(nodeLabel(demoted.id)));
    await t.pumpAndSettle();
    expect(find.text('ID FA4E00000103'), findsOneWidget);
    expect(icon('link strength, fragile'), findsOneWidget);
    // The row says the word; its help gives the numbers.
    expect(find.text('fragile'), findsOneWidget);
    expect(find.textContaining('dBm'), findsNothing);
    await t.tap(find.text('Link'));
    await t.pumpAndSettle();
    expect(
      find.text('This link: \u2212115 dBm, +3 dB over the noise floor.'),
      findsOneWidget,
    );
    Navigator.pop(t.element(find.byType(BottomSheet).last));
    await t.pumpAndSettle();
    expect(find.text('Demoted'), findsOneWidget);
    expect(find.text('its last hand-overs failed'), findsOneWidget);
    expect(icon('distance'), findsOneWidget);
    await closeSheet(t);
    await t.pumpAndSettle();

    // This node's own: the KNOWN row from the table read, no relays line.
    await t.tap(icon('this node'));
    await t.pumpAndSettle();
    expect(find.text('Relays known: 3'), findsNothing);
    expect(icon('neighbours known'), findsOneWidget);
    expect(icon('paired'), findsOneWidget);
    expect(icon('moving'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    debugPrint = print;
    await app.session.disconnect();
    await t.pump();
    app.disposeWithNode();
  });
}

/// Closes the open bottom sheet, which may cover the whole screen.
Future<void> closeSheet(WidgetTester t) async {
  Navigator.pop(t.element(find.byType(BottomSheet)));
  await t.pumpAndSettle();
}
