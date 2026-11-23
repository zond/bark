// What the node gives back, and why (spec "Phone link", Given back): busy
// and no neighbour go again by themselves, no way waits for the person,
// who may send it again as a new message or cancel it; and a value the
// node keeps is acted on once, however often it is read.

import 'dart:typed_data';

import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/ui/common.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'fake_node.dart';
import 'session_test.dart' show Phone;

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 23);
  final moved = Address(Position.fromDegrees(50.46, 30.53), [7, 7, 7, 7, 7, 7]);

  /// A phone on the fake node past its first rotation, which gives
  /// nothing back busy unless a test says so.
  void run(void Function(FakeAsync, Phone, FakeNode, DateTime Function()) b) {
    fakeAsync((async) {
      DateTime now() => start.add(async.elapsed);
      final p = Phone(sodium, now);
      async.flushMicrotasks();
      final node = FakeNode(now: now, sodium: sodium, app: p.me.identity)
        ..busyEvery = 0;
      p.session.attach(node.connect());
      async.flushMicrotasks();
      p.me.setNodeAddress(p.session.address!);
      async.elapse(FakeNode.firstRotation);
      b(async, p, node, now);
      node.dispose();
      p.session.dispose();
    });
  }

  Contact friendOf(Phone p, FakeNode node) =>
      p.contacts.byKey(hex(node.friend.identity))!;

  List<String> echoes(Session s) => [
    for (final r in s.received)
      if (r.text.startsWith('Echo')) r.text,
  ];

  int takes(Session s, int id) =>
      s.log.where((l) => l.text == 'Send ${msgIdHex(id)}: taken').length;

  test('busy: counts down its wait and goes again by itself', () {
    run((async, p, node, now) {
      final s = p.session;
      p.messenger.sendText(friendOf(p, node), 'a');
      async.elapse(const Duration(seconds: 1));
      final a = s.outbox.single;
      node.giveBack(a.id, Why.busy);
      async.flushMicrotasks();
      expect(a.state, OutState.givenBack);
      expect(a.why, Why.busy);
      expect(stateLabel(s, a), 'the network is busy, sending again in 0:26');
      async.elapse(FakeNode.givenBackWait + const Duration(seconds: 2));
      expect(takes(s, a.id), 2);
    });
  });

  test('no neighbour: sent again by itself once the node hears one, '
      'not before', () {
    run((async, p, node, now) {
      final s = p.session;
      p.messenger.sendText(friendOf(p, node), 'a');
      async.flushMicrotasks();
      // Held by the node, which loses its last neighbour before its first
      // hop takes it.
      async.elapse(const Duration(milliseconds: 50));
      final a = s.outbox.single;
      expect(a.state, OutState.taken);
      node.loseNeighbours(held: a.id);
      async.flushMicrotasks();
      expect(a.state, OutState.givenBack);
      expect(a.why, Why.noNeighbour);
      expect(a.retryAt, isNull);
      expect(s.status!.ready, isFalse);
      expect(
        textLabel(s, [a]),
        'your node hears no other node, sending when it does',
      );
      async.elapse(const Duration(minutes: 10));
      expect(a.state, OutState.givenBack);
      expect(takes(s, a.id), 1);
      node.hearNeighbour();
      async.elapse(const Duration(seconds: 1));
      expect(takes(s, a.id), 2);
      async.elapse(FakeNode.echoAfter * 2);
      expect(echoes(s), ['Echo: a']);
    });
  });

  test('no way: marked, never sent again by itself; Send again is a new '
      'message, which arrives', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, t) => t?.text == 'lost';
      p.messenger.sendText(f, 'lost');
      async.elapse(const Duration(seconds: 1));
      final o = s.outbox.single;
      expect(o.noWay, isTrue);
      expect(textLabel(s, [o]), 'found no way there');
      async.elapse(const Duration(hours: 1));
      expect(o.noWay, isTrue);
      expect(takes(s, o.id), 1);
      expect(echoes(s), isEmpty);

      // To the same address the box would be the same bytes, the same
      // message: it goes as a new text, with a new `sent`.
      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, [o], f.unasked(now())!);
      async.elapse(Session.turnWait * 2);
      final again = s.outbox.single;
      expect(again.id, isNot(o.id));
      expect(again.sent, greaterThan(o.sent));
      expect(again.body, 'lost');
      expect(again.state, OutState.taken);
      expect(again.replaced, [o.id]);
      async.elapse(FakeNode.echoAfter * 2);
      expect(echoes(s), ['Echo: lost']);
    });
  });

  test('no way, Send again from another node: the same text, its `sent` '
      'kept, another ID', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, t) => t?.text == 'lost';
      p.messenger.sendText(f, 'lost');
      async.elapse(const Duration(seconds: 1));
      final o = s.outbox.single;
      expect(o.noWay, isTrue);
      // The header names the node the phone writes from, which changed.
      p.me.setNodeAddress(moved);
      async.flushMicrotasks();
      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, [o], f.unasked(now())!);
      async.elapse(Session.turnWait * 2);
      final again = s.outbox.single;
      expect(again.id, isNot(o.id));
      expect(again.sent, o.sent);
      expect(again.at, o.at);
      expect(again.state, OutState.taken);
      async.elapse(FakeNode.echoAfter * 2);
      expect(echoes(s), ['Echo: lost']);
    });
  });

  test('a long text with one part given back for no way: marked, and Send '
      'again sends that part alone, its number kept', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      final long = List.filled(60, 'word').join(' ');
      node.noWayFor = (_, t) => t != null && t.part == 2;
      p.messenger.sendText(f, long);
      async.elapse(const Duration(seconds: 5));
      final [one, two] = s.outbox;
      expect([one.part, two.part], [1, 2]);
      expect(one.state, OutState.taken);
      expect(two.noWay, isTrue);
      expect(textLabel(s, s.outbox), '1 of 2 sent, part 2: found no way there');

      p.me.setNodeAddress(moved);
      async.flushMicrotasks();
      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, [one, two], f.unasked(now())!);
      async.elapse(Session.turnWait * 2);
      final [first, again] = s.outbox;
      expect(first, same(one));
      expect(again.id, isNot(two.id));
      expect([again.part, again.of, again.sent], [2, 2, two.sent]);
      expect(again.state, OutState.taken);
      // The fake friend echoes a text once it has every part.
      async.elapse(FakeNode.echoAfter * 3);
      final echo = s.received.where((r) => r.text.startsWith('Echo')).first;
      expect(s.partsOf(echo).map((r) => r?.text).join(), 'Echo: $long');
    });
  });

  test('a long text whose part would be the same message again goes whole, '
      'as a new text in its place', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      final long = List.filled(60, 'word').join(' ');
      node.noWayFor = (_, t) => t != null && t.part == 2;
      p.messenger.sendText(f, long);
      async.elapse(const Duration(seconds: 5));
      final old = [...s.outbox];
      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, old, f.unasked(now())!);
      async.elapse(const Duration(seconds: 5));
      expect(s.outbox.map((o) => o.part), [1, 2]);
      expect(s.outbox.every((o) => o.sent > old.last.sent), isTrue);
      expect(s.outbox.first.replaced, [for (final o in old) o.id]);
      expect(s.outbox.every((o) => o.state == OutState.taken), isTrue);
      // A late no way for the part that had gone is no stranger's.
      node.giveBack(old.first.id, Why.noWay);
      async.flushMicrotasks();
      expect(s.notice, isNull);
      expect(s.outbox.every((o) => o.state == OutState.taken), isTrue);
    });
  });

  test('Cancel removes a text given back for no way; its value read again '
      'at a reconnect and after a restart changes nothing', () {
    run((async, p, node, now) {
      final s = p.session;
      s.setPaired(nodeName(FakeNode.id));
      node.noWayFor = (_, _) => true;
      p.messenger.sendText(friendOf(p, node), 'lost');
      async.elapse(const Duration(seconds: 1));
      final o = s.outbox.single;
      expect(o.noWay, isTrue);
      s.drop(o);
      async.flushMicrotasks();
      expect(s.outbox, isEmpty);

      s.disconnect();
      s.attach(node.connect());
      async.flushMicrotasks();
      expect(s.givenBack!.msgId, o.id);
      expect(s.notice, isNull);
      expect(s.outbox, isEmpty);

      s.disconnect();
      final again = p.again(sodium, now);
      async.flushMicrotasks();
      again.session.attach(node.connect());
      async.flushMicrotasks();
      expect(again.session.givenBack!.msgId, o.id);
      expect(again.session.notice, isNull);
      again.session.dispose();
    });
  });

  test('a late no way for an earlier text marks it, not the latest; read '
      'again after a reconnect it changes nothing', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      p.messenger.sendText(f, 'a');
      async.elapse(const Duration(seconds: 1));
      p.messenger.sendText(f, 'b');
      async.elapse(const Duration(minutes: 5));
      final [a, b] = s.outbox;
      expect(echoes(s), ['Echo: a', 'Echo: b']);
      node.giveBack(a.id, Why.noWay);
      async.flushMicrotasks();
      expect(a.noWay, isTrue);
      expect(b.state, OutState.taken);

      // Sent again, it goes; the value stays until the node takes it, and
      // the one read before that is not taken for new.
      p.messenger.sendAgain(f, [a], f.unasked(now())!);
      async.flushMicrotasks();
      s.disconnect();
      s.attach(node.connect());
      async.flushMicrotasks();
      expect(s.notice, isNull);
      expect(s.outbox.where((o) => o.noWay), isEmpty);
      async.elapse(const Duration(seconds: 2));
      expect(s.outbox.every((o) => o.state == OutState.taken), isTrue);
    });
  });

  test('a message this app did not send is told once', () {
    run((async, p, node, now) {
      final s = p.session;
      node.giveBack(0x12345678, Why.noWay);
      async.flushMicrotasks();
      expect(s.notice!.text, contains('12345678'));
      s.dismissNotice();
      s.disconnect();
      s.attach(node.connect());
      async.flushMicrotasks();
      expect(s.notice, isNull);
    });
  });

  test('a card, a reaction and a home given back for no way are told', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, _) => true;
      p.messenger.sendCard(f);
      async.elapse(const Duration(seconds: 1));
      expect(s.outbox.last.noWay, isTrue);
      expect(
        s.notice!.text,
        'Your card did not reach Fake friend: found no '
        'way there.',
      );
      p.messenger.react(s.received.first, '👍');
      async.elapse(const Duration(seconds: 2));
      expect(
        s.notice!.text,
        'Your reaction 👍 did not reach Fake friend: '
        'found no way there.',
      );
      expect(
        s.log.map((l) => l.text),
        contains(
          'Your reaction 👍 did not reach Fake friend: found no way '
          'there.',
        ),
      );
      // Nothing of theirs goes again by itself.
      async.elapse(const Duration(hours: 1));
      expect(s.outbox.every((o) => o.noWay), isTrue);
      expect(s.outbox.every((o) => takes(s, o.id) == 1), isTrue);
    });
  });

  test('the stored outbox keeps why; one given back before the node said '
      'why is read as busy', () {
    final o = Outgoing(
      Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]),
      Uint8List.fromList([0x20, 1, 2, 3]),
      at: start,
      kind: OutKind.text,
      sent: 7,
      body: 'x',
      state: OutState.givenBack,
      why: Why.noWay,
      replaced: const [5, 6],
    );
    final j = o.toJson();
    expect(j['why'], 'noWay');
    final back = Outgoing.fromJson(j);
    expect(
      [back.state, back.why, back.replaced],
      [
        OutState.givenBack,
        Why.noWay,
        [5, 6],
      ],
    );
    final old = Map<String, dynamic>.of(j)
      ..remove('why')
      ..remove('replaced');
    final read = Outgoing.fromJson(old);
    expect([read.why, read.replaced], [Why.busy, isEmpty]);
    // Taken, it has no why.
    expect(Outgoing.fromJson({...old, 'state': 'taken'}).why, isNull);
  });

  test('why, and what the node gave back, are kept across a restart', () {
    run((async, p, node, now) {
      final s = p.session;
      s.setPaired(nodeName(FakeNode.id));
      node.noWayFor = (_, _) => true;
      p.messenger.sendText(friendOf(p, node), 'lost');
      async.elapse(const Duration(seconds: 1));
      final again = p.again(sodium, now);
      async.flushMicrotasks();
      expect(again.session.outbox.single.why, Why.noWay);
      expect(
        again.session.nodes[nodeName(FakeNode.id)]!.givenBackSeen,
        s.outbox.single.id,
      );
      again.session.dispose();
    });
  });
}
