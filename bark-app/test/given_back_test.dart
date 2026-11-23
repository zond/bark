// What the node gives back, and why (spec "Phone link", Given back): busy
// and no neighbour go again by themselves, no way waits for the person,
// who may send it again, whole, as a new text, or cancel it; and a value
// the node keeps is acted on once, however often it is read.

import 'dart:typed_data';

import 'package:bark_app/card.dart';
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
  final theirHome = Address(Position.fromDegrees(50.47, 30.5), [
    8,
    8,
    8,
    8,
    8,
    8,
  ]);
  final long = List.filled(90, 'word').join(' ');

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

  /// The texts the fake friend echoed, each whole.
  List<String> echoes(Session s) => [
    for (final r in s.received)
      if (r.part == 1 && r.text.startsWith('Echo'))
        s.partsOf(r).map((x) => x?.text ?? '').join(),
  ];

  int takes(Session s, int id) =>
      s.log.where((l) => l.text == 'Send ${msgIdHex(id)}: taken').length;

  /// Long enough for every queued message to go, through the waits of
  /// writes the fake answers not ready.
  const settle = Duration(seconds: 30);

  void reconnect(FakeAsync async, Session s, FakeNode node) {
    s.disconnect();
    async.flushMicrotasks();
    s.attach(node.connect());
    async.flushMicrotasks();
  }

  test('decoding: 7 bytes; 6 are malformed, 8 read as 7, an unknown why '
      'malformed', () {
    final g = GivenBack.decode([0x35, 0x7b, 0x60, 0x10, 0, 200, 2, 0xaa])!;
    expect([g.msgId, g.waitS, g.why], [0x357b6010, 200, Why.noWay]);
    expect(
      () => GivenBack.decode([0x35, 0x7b, 0x60, 0x10, 0, 200]),
      throwsA(isA<Malformed>()),
    );
    expect(
      () => GivenBack.decode([0, 0, 0, 1, 0, 0, 3]),
      throwsA(isA<Malformed>()),
    );
  });

  test('busy: counts down its wait and goes again by itself', () {
    run((async, p, node, now) {
      final s = p.session;
      p.messenger.sendText(friendOf(p, node), 'a');
      async.elapse(const Duration(seconds: 1));
      final a = s.outbox.single;
      node.giveBack(a.id, Why.busy);
      async.flushMicrotasks();
      expect((a.state, a.why), (OutState.givenBack, Why.busy));
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
      expect((a.state, a.why), (OutState.givenBack, Why.noNeighbour));
      expect(a.retryAt, isNull);
      expect(
        textLabel(s, [a]),
        'the node hears no other, sending when it does',
      );
      async.elapse(const Duration(minutes: 10));
      expect(a.state, OutState.givenBack);
      expect(node.writesOf[a.id], 1);
      node.hearNeighbour();
      async.elapse(const Duration(seconds: 1));
      expect(takes(s, a.id), 2);
      async.elapse(FakeNode.echoAfter * 2);
      expect(echoes(s), ['Echo: a']);
    });
  });

  test('no neighbour while it still hears some, none of which took it: '
      'written no more often than the node allows, and once when ready', () {
    run((async, p, node, now) {
      final s = p.session;
      p.messenger.sendText(friendOf(p, node), 'a');
      async.elapse(const Duration(milliseconds: 50));
      final a = s.outbox.single;
      // Past its hand-over, after which the fake would take messages.
      async.elapse(const Duration(seconds: 1));
      const wait = Duration(seconds: 30);
      node.noneTookIt(a.id, wait);
      async.elapse(wait - const Duration(milliseconds: 100));
      // It counts neighbours heard, so the app asks for the turn, and asks
      // again only after a turn's wait (spec "Phone link", Sending).
      final asked = node.writesOf[a.id]! - 1;
      expect(asked, lessThanOrEqualTo(wait.inMilliseconds ~/ 12640 + 1));
      expect(takes(s, a.id), 1);
      async.elapse(Session.turnWait);
      expect(takes(s, a.id), 2);
      expect(node.writesOf[a.id], asked + 2);
    });
  });

  test('no way: marked, never sent again by itself; Send again is a new '
      'text, which arrives', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, t) => t?.text == 'lost';
      p.messenger.sendText(f, 'lost');
      async.elapse(const Duration(seconds: 1));
      final o = s.outbox.single;
      expect(o.noWay, isTrue);
      expect(textLabel(s, [o]), 'found no way there');
      expect(s.notice!.text, 'Your text to Fake friend found no way there.');
      async.elapse(const Duration(hours: 1));
      expect(o.noWay, isTrue);
      expect(node.writesOf[o.id], 1);
      expect(echoes(s), isEmpty);

      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, [o], f.unasked(now())!);
      async.elapse(settle);
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

  test('the same bytes again after a no way go nowhere', () {
    run((async, p, node, now) {
      final s = p.session;
      node.noWayFor = (_, _) => true;
      p.messenger.sendText(friendOf(p, node), 'lost');
      async.elapse(const Duration(seconds: 1));
      final o = s.outbox.single;
      expect(o.noWay, isTrue);
      node.noWayFor = (_, _) => false;
      o.requeue();
      node.changed();
      async.elapse(settle);
      expect(o.state, OutState.taken);
      async.elapse(FakeNode.echoAfter * 2);
      expect(echoes(s), isEmpty);
    });
  });

  test('there, home, there again: three messages, each with its own ID', () {
    run((async, p, node, now) {
      final s = p.session;
      var f = friendOf(p, node);
      f = f.withCard(
        PersonCard(f.card.identity, theirHome, f.card.time + 1, f.card.name),
      );
      p.contacts.put(f);
      async.flushMicrotasks();
      node.noWayFor = (_, _) => true;
      p.messenger.sendText(f, 'lost', toward: Toward.there);
      async.elapse(settle);
      final ids = <int>[s.outbox.single.id];
      for (final t in [Toward.home, Toward.there]) {
        p.messenger.sendAgain(f, [...s.outbox], t);
        async.elapse(settle);
        expect(s.outbox.single.noWay, isTrue);
        ids.add(s.outbox.single.id);
      }
      expect(ids.toSet(), hasLength(3));
      expect(s.outbox.single.replaced, [ids[1], ids[0]]);
      expect(s.outbox.single.dst.toString(), f.place.toString());
    });
  });

  test('a long text with one part given back for no way: marked; Send again '
      'sends the whole text anew and nothing of the old one', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, t) => t != null && t.part == 2;
      p.messenger.sendText(f, long);
      async.elapse(settle);
      final old = [...s.outbox];
      expect(old.map((o) => o.part), [1, 2, 3]);
      expect(old[1].noWay, isTrue);
      expect(textLabel(s, old), '2 of 3 sent, part 2: found no way there');
      // Part 3 as when its answer was lost: back in the queue.
      old[2].state = OutState.queued;
      final writes = {for (final o in old) o.id: node.writesOf[o.id]};
      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, old, f.unasked(now())!);
      expect(s.outbox.where(old.contains), isEmpty);
      async.elapse(settle);
      for (final o in old) {
        expect(node.writesOf[o.id], writes[o.id], reason: 'part ${o.part}');
      }
      expect(s.outbox.map((o) => o.part), [1, 2, 3]);
      expect(s.outbox.every((o) => o.sent > old.last.sent), isTrue);
      expect(s.outbox.first.replaced, [for (final o in old) o.id]);
      expect(s.outbox.every((o) => o.state == OutState.taken), isTrue);
      async.elapse(FakeNode.echoAfter * 3);
      expect(echoes(s), ['Echo: $long']);
      // A late no way for a part that had gone is no stranger's.
      node.giveBack(old.first.id, Why.noWay);
      async.flushMicrotasks();
      expect(s.notice?.text, isNot(contains('did not send')));
      expect(s.outbox.every((o) => o.state == OutState.taken), isTrue);
    });
  });

  test('two quick taps on Send again send once', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, _) => true;
      p.messenger.sendText(f, 'lost');
      async.elapse(const Duration(seconds: 1));
      final o = s.outbox.single;
      node.noWayFor = (_, _) => false;
      p.messenger.sendAgain(f, [o], Toward.there);
      p.messenger.sendAgain(f, [o], Toward.there);
      async.elapse(settle);
      expect(s.outbox, hasLength(1));
      async.elapse(FakeNode.echoAfter * 2);
      expect(echoes(s), ['Echo: lost']);
    });
  });

  test('Cancel on a long text with a part taken: its parts in words, not '
      'a check', () {
    run((async, p, node, now) {
      final s = p.session;
      node.noWayFor = (_, t) => t != null && t.part == 2;
      p.messenger.sendText(friendOf(p, node), long);
      async.elapse(settle);
      s.drop(s.outbox[1]);
      async.flushMicrotasks();
      expect(partsAgree(s.outbox), isFalse);
      expect(textLabel(s, s.outbox), '2 of 3 sent');
    });
  });

  test('the parts agree only for one reason', () {
    Outgoing part(int n, Why why) => Outgoing(
      theirHome,
      Uint8List.fromList([0x20, n]),
      at: start,
      kind: OutKind.text,
      sent: n,
      part: n,
      of: 2,
      state: OutState.givenBack,
      why: why,
    );
    expect(partsAgree([part(1, Why.noWay), part(2, Why.noWay)]), isTrue);
    expect(partsAgree([part(1, Why.noWay), part(2, Why.busy)]), isFalse);
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
      s.dismissNotice();
      s.drop(o);
      async.flushMicrotasks();
      expect(s.outbox, isEmpty);

      reconnect(async, s, node);
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
      'again after a reconnect it is known as read', () {
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
      reconnect(async, s, node);
      expect(
        s.log.map((l) => l.text),
        contains('Given back: ${msgIdHex(a.id)} read before'),
      );
      expect((a.noWay, b.state), (true, OutState.taken));
    });
  });

  test('a give-back for the same ID after the take that emptied it is '
      'acted on', () {
    run((async, p, node, now) {
      final s = p.session;
      p.messenger.sendText(friendOf(p, node), 'a');
      async.elapse(const Duration(seconds: 1));
      final a = s.outbox.single;
      node.giveBack(a.id, Why.busy);
      async.elapse(FakeNode.givenBackWait + const Duration(seconds: 2));
      expect(takes(s, a.id), 2);
      node.giveBack(a.id, Why.noWay);
      async.flushMicrotasks();
      expect(a.noWay, isTrue);
    });
  });

  test('given back while away: read before anything is written, so the '
      'text written meanwhile goes and the one given back is marked', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      p.messenger.sendText(f, 'x');
      async.elapse(const Duration(seconds: 1));
      final x = s.outbox.single;
      s.disconnect();
      async.flushMicrotasks();
      node.giveBack(x.id, Why.noWay);
      p.messenger.sendText(f, 'z');
      async.flushMicrotasks();
      s.attach(node.connect());
      async.elapse(settle);
      final z = s.outbox.last;
      expect(x.noWay, isTrue);
      expect(z.state, OutState.taken);
    });
  });

  test('an answer lost with the link: the give-back for the queued text '
      'marks it, and it is not written again', () {
    run((async, p, node, now) {
      final s = p.session;
      node.loseNextAnswer = true;
      p.messenger.sendText(friendOf(p, node), 'y');
      async.elapse(const Duration(seconds: 1));
      final y = s.outbox.single;
      expect(s.connected, isFalse);
      expect(y.state, OutState.queued);
      node.giveBack(y.id, Why.noWay);
      s.attach(node.connect());
      async.elapse(settle);
      expect(y.noWay, isTrue);
      expect(node.writesOf[y.id], 1);
    });
  });

  test('a guest is given back its message', () {
    run((async, p, node, now) {
      final guest = Phone(sodium, now);
      async.flushMicrotasks();
      final g = node.connectGuest();
      guest.session.attach(g, guest: true);
      async.flushMicrotasks();
      final o = Outgoing(
        theirHome,
        Uint8List.fromList([0x20, 1, 2, 3]),
        at: now(),
        kind: OutKind.text,
        sent: 1,
      );
      guest.session.queue(o);
      // Within the 30 s a guest's link lasts.
      async.elapse(const Duration(seconds: 5));
      expect(o.state, OutState.taken);
      node.giveBackTo(g, o.id, Why.noWay);
      async.flushMicrotasks();
      expect(o.noWay, isTrue);
      g.close();
      guest.session.dispose();
    });
  });

  test('a message this app did not send is told once', () {
    run((async, p, node, now) {
      final s = p.session;
      node.giveBack(0x12345678, Why.noWay);
      async.flushMicrotasks();
      expect(s.notice!.text, contains('12345678'));
      s.dismissNotice();
      reconnect(async, s, node);
      expect(s.notice, isNull);
    });
  });

  test('a card, a reaction and a home given back for no way are told, and '
      'not sent again', () {
    run((async, p, node, now) {
      final s = p.session;
      final f = friendOf(p, node);
      node.noWayFor = (_, _) => true;
      p.messenger.sendCard(f);
      async.elapse(const Duration(seconds: 1));
      expect(s.outbox.last.noWay, isTrue);
      expect(s.notice!.text, 'Your card found no way to Fake friend.');
      p.messenger.react(s.received.first, '👍');
      async.elapse(settle);
      expect(s.notice!.text, 'Your reaction 👍 found no way to Fake friend.');
      p.messenger.sendHome(f, (
        home: theirHome,
        key: Uint8List.fromList(List.filled(32, 3)),
      ));
      async.elapse(settle);
      expect(
        s.notice!.text,
        'The home you gave Fake friend found no way to them.',
      );
      async.elapse(const Duration(hours: 1));
      expect(s.outbox.every((o) => o.noWay), isTrue);
      expect(s.outbox.every((o) => node.writesOf[o.id] == 1), isTrue);
    });
  });

  test('the stored outbox keeps why; one given back before the node said '
      'why is read as busy', () {
    final o = Outgoing(
      theirHome,
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
