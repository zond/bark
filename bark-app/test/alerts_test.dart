// What notifies: each new text, collected per contact until its thread is
// opened, a text in parts once, and nothing for a thread in front of the
// person, a reaction or a card; the link's notification with a
// Bluetooth link only; starting with the phone while the paired node is this
// phone's and the person has not stopped looking; the permission asked
// once, at the first connection; the battery exemption in Settings; a
// tapped notification's thread; and a new text bringing an archived
// conversation back.

import 'dart:async';
import 'dart:typed_data';

import 'package:bark_app/alerts.dart';
import 'package:bark_app/app.dart';
import 'package:bark_app/app_layer.dart';
import 'package:bark_app/contacts.dart';

import 'fake_ble.dart';
import 'fake_node.dart';

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

  /// What the app is to start with the phone under, each time it changed.
  final List<String?> boot = [];
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
  Future<void> hold(Held held) async => log.add('hold ${held.title}');
  @override
  Future<void> letGo() async => log.add('let go');
  @override
  Future<void> atBoot(Held? held) async => boot.add(held?.title);
  @override
  Future<void> texts(
    String key,
    String name,
    List<Unread> texts, {
    bool quiet = false,
  }) async => log.add(
    '${quiet ? '(quiet) ' : ''}$name: ${texts.map((t) => t.text).join(' | ')}',
  );
  @override
  Future<void> clear(String key) async => log.add('clear $key');
  @override
  Stream<String> get tapped => taps.stream;
  @override
  Future<bool?> batteryExempt() async => exempt;
  @override
  Future<void> askBatteryExempt() async => log.add('ask exempt');
  @override
  Future<void> openLink(String url) async => log.add('open $url');
  @override
  Future<void> openBluetoothSettings() async => log.add('Bluetooth settings');
  @override
  Future<void> share(String text, Uint8List png) async =>
      log.add('share $text');
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
      app.connectFake();
      async.flushMicrotasks();
      body(async, app, n);
      app.session.disconnect();
      async.flushMicrotasks();
      app.disposeWithNode();
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
      sealBox(pair, 0, sodium.randombytes.buf(nonceLen), writePlain(h, rs)),
    );
  }

  test('texts collect per contact until their thread is opened', () {
    run((async, app, n) {
      expect(n.log, [
        // The first state goes out even when nothing is held: the app
        // may have started with the phone, under the link's notification.
        'let go',
        'hold Connected to ID FA4E00000001, paired',
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

  test('a text in parts notifies once, then shows each part quietly', () {
    run((async, app, n) {
      final f = friend(app);
      app.alerts.opened(f);
      app.alerts.closed(f);
      async.flushMicrotasks();
      n.log.clear();
      void part(int k, String t) => app.messenger.receive(
        fromFriend(app, 50 + k - 1, [TextRecord(t, k, 3)]),
      );
      part(2, 'two ');
      async.flushMicrotasks();
      part(1, 'one ');
      async.flushMicrotasks();
      app.messenger.receive(fromFriend(app, 60, [const TextRecord('other')]));
      async.flushMicrotasks();
      part(3, 'three');
      async.flushMicrotasks();
      expect(n.log, [
        'Fake friend: part 1 of 3 not yet here\ntwo \n'
            'part 3 of 3 not yet here',
        '(quiet) Fake friend: one two \npart 3 of 3 not yet here',
        'Fake friend: one two \npart 3 of 3 not yet here | other',
        '(quiet) Fake friend: one two three | other',
      ]);
    });
  });

  test('a reaction, a card and what opens for nobody notify '
      'nothing', () {
    run((async, app, n) {
      n.log.clear();
      app.messenger.sendText(app.contacts.byKey(friend(app))!, 'a');
      async.elapse(const Duration(seconds: 6));
      n.log.clear();
      final a = app.session.outbox.first;
      app.messenger.receive(fromFriend(app, 1, [ReactionRecord(a.sent, '❤️')]));
      async.flushMicrotasks();
      expect(a.reaction, '❤️');
      final card = app.fakeNode!.friendCard;
      app.messenger.receive(fromFriend(app, 2, [CardRecord(card.encode())]));
      app.messenger.receive(InboxMessage(3, [9, 9, 9]));
      async.flushMicrotasks();
      expect(app.session.received.last.from, isNull);
      expect(n.log, isEmpty);
    });
  });

  test('a new text brings an archived conversation back and notifies; an '
      'reaction does not', () {
    run((async, app, n) {
      final f = friend(app);
      app.alerts.opened(f);
      app.alerts.closed(f);
      final c = app.contacts.byKey(f)!;
      app.contacts.archiveContact(c);
      async.flushMicrotasks();
      n.log.clear();
      app.messenger.sendText(app.contacts.byKey(f)!, 'a');
      async.elapse(const Duration(seconds: 6));
      // The echo is a text: back, with its notification.
      expect(app.contacts.isArchived(f), isFalse);
      expect(app.contacts.byKey(f)!.archived, isFalse);
      expect(n.log.last, 'Fake friend: Echo: a');

      app.contacts.archiveConversation(f);
      async.flushMicrotasks();
      final a = app.session.outbox.first;
      app.messenger.receive(fromFriend(app, 1, [ReactionRecord(a.sent, '👍')]));
      async.flushMicrotasks();
      expect(app.contacts.isArchived(f), isTrue);
      app.messenger.receive(fromFriend(app, 2, [const TextRecord('back')]));
      async.flushMicrotasks();
      expect(app.contacts.isArchived(f), isFalse);
      expect(n.log.last, 'Fake friend: Echo: a | back');
    });
  });

  test('the link notification is up while a link is', () {
    run((async, app, n) {
      expect(n.log, contains('hold Connected to ID FA4E00000001, paired'));
      n.log.clear();
      app.session.disconnect();
      async.flushMicrotasks();
      expect(n.log, ['let go']);
      app.connectFake();
      async.flushMicrotasks();
      expect(n.log, ['let go', 'hold Connected to ID FA4E00000001, paired']);
    });
  });

  test('the app starts with the phone while the paired node is its own '
      'and the person has not stopped looking', () {
    run((async, app, n) {
      const looking =
          'Not connected. Looking for ID FA4E00000001; texts wait on the phone';
      expect(n.boot.last, looking);
      app.disconnect();
      async.flushMicrotasks();
      expect(n.boot.last, isNull);
      app.connectFake();
      async.flushMicrotasks();
      expect(n.boot.last, looking);

      // The stop is kept: the app started again does not look, nor
      // start with the phone, and lets go of a service started with it.
      app.disconnect();
      async.flushMicrotasks();
      final n2 = FakeNotifier();
      final again = AppModel(
        sodium: sodium,
        me: Me(MemoryStore(), sodium, app.now),
        session: Session(store: app.session.store, now: app.now),
        contacts: Contacts(MemoryStore()),
        now: app.now,
        notifier: n2,
      );
      again.load();
      async.flushMicrotasks();
      expect(again.session.stopped, isTrue);
      expect(n2.boot, [null]);
      expect(n2.log, ['let go']);
      again.dispose();

      // Nor once the node is another phone's.
      app.connectFake();
      async.flushMicrotasks();
      expect(n.boot.last, looking);
      app.session.disconnect();
      app.fakeNode!.pairElsewhere();
      async.flushMicrotasks();
      app.connectFake().catchError((_) {});
      async.flushMicrotasks();
      expect(app.session.nodes.lost, contains(nodeName(FakeNode.id)));
      expect(n.boot.last, isNull);
    });
  });

  /// The person taps their paired node's row in Settings.
  Future<void> connectFake(WidgetTester t) async {
    await t.tap(find.text('ID FA4E00000001'));
    await t.pumpAndSettle();
  }

  testWidgets('notifications are asked for once, at the first connection', (
    t,
  ) async {
    final n = FakeNotifier(notify: false, exempt: false);
    final app = model(n, DateTime.now);
    await t.runAsync(app.load);
    app.fakeNode = FakeNode(now: app.now, sodium: sodium, app: app.me.identity);
    await app.session.setPaired(nodeName(FakeNode.id));
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();

    // The battery exemption.
    expect(find.textContaining('may close bark'), findsOneWidget);
    await t.ensureVisible(find.text('Let bark run'));
    await t.tap(find.text('Let bark run'));
    expect(n.log.last, 'ask exempt');
    await t.ensureVisible(find.text('ID FA4E00000001'));

    await connectFake(t);
    expect(find.text('Notifications'), findsOneWidget);
    await t.tap(find.text('Continue'));
    await t.pumpAndSettle();
    expect(n.asked, 1);
    expect(app.me.askedToNotify, isTrue);
    expect(find.text('ID FA4E00000001'), findsOneWidget);

    await app.session.disconnect();
    await t.pump();
    n.notify = false;
    await connectFake(t);
    expect(find.text('Notifications'), findsNothing);
    expect(n.asked, 1);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.disposeWithNode();
  });

  testWidgets('a tapped notification opens its thread', (t) async {
    final n = FakeNotifier();
    final app = model(n, DateTime.now);
    await t.runAsync(app.load);
    await t.pumpWidget(BarkApp(app));
    await app.connectFake();
    await t.pumpAndSettle();
    n.taps.add(friend(app));
    await t.pumpAndSettle();
    expect(find.textContaining('Not verified yet'), findsOneWidget);
    expect(n.log.last, 'clear ${friend(app)}');

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.disposeWithNode();
  });
}
