// What notifies: each new text, collected per contact until its thread is
// opened, and nothing for a thread in front of the person, an
// acknowledgement or a card; the link's notification with a Bluetooth
// link only; the permission asked once, at the first connection; the
// battery exemption in Settings; and a tapped notification's thread.

import 'dart:async';

import 'package:bark_app/alerts.dart';
import 'package:bark_app/app.dart';
import 'package:bark_app/app_layer.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// Writes down what the app asks of the phone.
class FakeNotifier implements Notifier {
  final List<String> log = [];
  bool notify;
  bool exempt;
  int asked = 0;
  final taps = StreamController<String>();
  FakeNotifier({this.notify = true, this.exempt = true});

  @override
  Future<bool> allowed() async => notify;
  @override
  Future<bool> ask() async {
    asked++;
    return notify = true;
  }

  @override
  Future<void> linkUp(String node) async => log.add('up $node');
  @override
  Future<void> linkDown() async => log.add('down');
  @override
  Future<void> texts(String key, String name, List<Received> texts) async =>
      log.add('$name: ${texts.map((t) => t.body).join(' | ')}');
  @override
  Future<void> clear(String key) async => log.add('clear $key');
  @override
  Stream<String> get tapped => taps.stream;
  @override
  Future<bool?> batteryExempt() async => exempt;
  @override
  Future<void> askBatteryExempt() async => log.add('ask exempt');
}

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());

  AppModel model(FakeNotifier n, DateTime Function() now) => AppModel(
    sodium: sodium,
    me: Me(MemoryStore(), sodium, now),
    session: Session(store: MemoryStore(), now: now),
    contacts: Contacts(MemoryStore()),
    now: now,
    notifier: n,
  );

  /// The app connected to the fake node, which has given it the fake
  /// friend's two first messages.
  void run(void Function(FakeAsync, AppModel, FakeNotifier) body) {
    fakeAsync((async) {
      final start = DateTime.utc(2026, 11, 6);
      final n = FakeNotifier();
      final app = model(n, () => start.add(async.elapsed));
      app.load();
      async.flushMicrotasks();
      app.setFake(true);
      app.connectFake();
      async.flushMicrotasks();
      body(async, app, n);
      app.session.disconnect();
      async.flushMicrotasks();
      app.dispose();
    });
  }

  String friend(AppModel app) => hex(app.fakeNode!.friend.identity);

  /// A boxed message from the fake friend.
  InboxMessage fromFriend(AppModel app, int id, List<Record> rs) {
    final f = app.fakeNode!.friend;
    final pair = sodium.crypto.box.precalculate(
      publicKey: app.me.identity,
      secretKey: f.pair.secretKey,
    );
    final h = Header(greater(f.identity, app.me.identity), 1000 + id, null);
    return InboxMessage(
      id,
      sealBox(pair, sodium.randombytes.buf(nonceLen), writePlain(h, rs)),
    );
  }

  test('texts collect per contact until their thread is opened', () {
    run((async, app, n) {
      expect(n.log, [
        'Fake friend: Hello from the fake node.',
        'Fake friend: Hello from the fake node. | '
            'A second message, waiting in the inbox.',
      ]);
      n.log.clear();
      final f = friend(app);
      app.alerts.opened(f);
      async.flushMicrotasks();
      expect(n.log, ['clear $f']);

      // In front of the person, the echo notifies nothing.
      n.log.clear();
      app.messenger.sendText(app.contacts.byKey(f)!, 'one');
      async.elapse(const Duration(seconds: 6));
      expect(app.session.received.last.body, 'Echo: one');
      expect(n.log, isEmpty);

      // The app behind another, the thread still open: it notifies, from
      // the first unread on.
      app.alerts.front(false);
      app.messenger.sendText(app.contacts.byKey(f)!, 'two');
      async.elapse(const Duration(seconds: 6));
      expect(n.log, ['Fake friend: Echo: two']);

      // Back in front of the open thread, it is cleared.
      app.alerts.front(true);
      async.flushMicrotasks();
      expect(n.log.last, 'clear $f');

      // Closed, it notifies again.
      n.log.clear();
      app.alerts.closed(f);
      app.messenger.receive(fromFriend(app, 9, [TextRecord('three')]));
      async.flushMicrotasks();
      expect(n.log, ['Fake friend: three']);
    });
  });

  test('an acknowledgement, a card and what opens for nobody notify '
      'nothing', () {
    run((async, app, n) {
      n.log.clear();
      app.messenger.sendText(app.contacts.byKey(friend(app))!, 'a');
      async.elapse(const Duration(seconds: 6));
      n.log.clear();
      final a = app.session.outbox.first;
      app.messenger.receive(fromFriend(app, 1, [AckRecord(a.sent)]));
      async.flushMicrotasks();
      expect(a.acked, isTrue);
      final card = app.fakeNode!.friendCard;
      app.messenger.receive(fromFriend(app, 2, [CardRecord(card.encode())]));
      app.messenger.receive(InboxMessage(3, [9, 9, 9]));
      async.flushMicrotasks();
      expect(app.session.received.last.from, isNull);
      expect(n.log, isEmpty);
    });
  });

  test('the link notification is up while a Bluetooth link is', () {
    run((async, app, n) {
      // The fake node is no connected device.
      expect(n.log.where((l) => l.startsWith('up')), isEmpty);
      app.session.disconnect();
      async.flushMicrotasks();

      // A link that is not the fake, as Bluetooth's is.
      app.setFake(false);
      final node = FakeNode(now: app.now, sodium: sodium, app: app.me.identity);
      app.session.attach(node.connect());
      async.flushMicrotasks();
      n.log.clear();
      app.session.disconnect();
      async.flushMicrotasks();
      expect(n.log, ['down']);
      app.session.attach(node.connect());
      async.flushMicrotasks();
      expect(n.log, ['down', 'up bark-fa4e00000001']);
      node.dispose();
    });
  });

  Future<void> connectFake(WidgetTester t) async {
    await t.tap(find.text('Connect'));
    await t.pumpAndSettle();
  }

  testWidgets('notifications are asked for once, at the first connection', (
    t,
  ) async {
    final n = FakeNotifier(notify: false, exempt: false);
    final app = model(n, DateTime.now);
    await t.runAsync(app.load);
    app.setFake(true);
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();

    // The battery exemption.
    expect(find.textContaining('not exempt'), findsOneWidget);
    await t.tap(find.text('Exempt bark'));
    expect(n.log, ['ask exempt']);

    await connectFake(t);
    expect(find.text('Notifications'), findsOneWidget);
    await t.tap(find.text('Continue'));
    await t.pumpAndSettle();
    expect(n.asked, 1);
    expect(app.me.askedToNotify, isTrue);
    expect(find.text('bark-fa4e00000001'), findsOneWidget);

    await app.session.disconnect();
    await t.pump();
    n.notify = false;
    await connectFake(t);
    expect(find.text('Notifications'), findsNothing);
    expect(n.asked, 1);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('a tapped notification opens its thread', (t) async {
    final n = FakeNotifier();
    final app = model(n, DateTime.now);
    await t.runAsync(app.load);
    await t.pumpWidget(BarkApp(app));
    app.setFake(true);
    await app.connectFake();
    await t.pumpAndSettle();
    n.taps.add(friend(app));
    await t.pumpAndSettle();
    expect(find.textContaining('Not verified yet'), findsOneWidget);
    expect(n.log.last, 'clear ${friend(app)}');

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });
}
