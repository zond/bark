// The app's side of the link against the fake node: the queue, the
// inbox, given back and the version check; and what is sealed and
// opened on top of them: texts, acknowledgements, first messages and
// the mailbox's replies; and the node paired with, and whether the person
// stopped looking for it, kept across a restart.

import 'dart:typed_data';

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/conversations.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/messenger.dart';
import 'package:bark_app/node_link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

class NewerNode extends FakeNode {
  NewerNode({required super.now, required super.sodium, required super.app});
  @override
  Future<NodeInfo> readNode() async =>
      NodeInfo(linkVersion + 1, FakeNode.id, 1);
}

/// A node at link version 2, before Home: still attached, and given the
/// clock.
class Version2Node extends FakeNode {
  Version2Node({required super.now, required super.sodium, required super.app});
  @override
  Future<NodeInfo> readNode() async => NodeInfo(2, FakeNode.id, 1);
}

/// One phone: its person, contacts, session and messenger, with stores
/// that outlive it for the restart tests.
class Phone {
  final MemoryStore meStore, contactStore, sessionStore;
  late final Me me;
  late final Contacts contacts;
  late final Session session;
  late final Messenger messenger;

  Phone(
    Sodium s,
    DateTime Function() now, {
    MemoryStore? meStore,
    MemoryStore? contactStore,
    MemoryStore? sessionStore,
  }) : meStore = meStore ?? MemoryStore(),
       contactStore = contactStore ?? MemoryStore(),
       sessionStore = sessionStore ?? MemoryStore() {
    me = Me(this.meStore, s, now);
    contacts = Contacts(this.contactStore);
    session = Session(store: this.sessionStore, now: now);
    messenger = Messenger(s, me, contacts, session, now);
    me.load();
    contacts.load();
    session.load();
  }

  Phone again(Sodium s, DateTime Function() now) => Phone(
    s,
    now,
    meStore: meStore,
    contactStore: contactStore,
    sessionStore: sessionStore,
  );
}

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 5);
  final elsewhere = Address(Position.fromDegrees(50.0, 30.0), [
    1,
    2,
    3,
    4,
    5,
    6,
  ]);

  void run(void Function(FakeAsync, Phone, FakeNode) body) {
    fakeAsync((async) {
      DateTime now() => start.add(async.elapsed);
      final p = Phone(sodium, now);
      async.flushMicrotasks();
      final node = FakeNode(now: now, sodium: sodium, app: p.me.identity);
      p.session.attach(node.connect());
      async.flushMicrotasks();
      // As the app model does: the card carries the node's address.
      p.me.setNodeAddress(p.session.address!);
      async.flushMicrotasks();
      body(async, p, node);
      node.dispose();
      p.session.dispose();
    });
  }

  Contact friendOf(Phone p, FakeNode node) =>
      p.contacts.byKey(hex(node.friend.identity))!;

  /// The box key between the phone and the fake friend.
  PrecalculatedBox friendPair(Phone p, FakeNode node) =>
      sodium.crypto.box.precalculate(
        publicKey: p.me.identity,
        secretKey: node.friend.pair.secretKey,
      );

  /// A boxed message from the fake friend to the phone.
  Uint8List fromFriend(
    FakeNode node,
    Phone p,
    int sent,
    List<Record> rs, [
    Address? from,
  ]) => sealBox(
    friendPair(p, node),
    0,
    sodium.randombytes.buf(nonceLen),
    writePlain(
      Header(greater(node.friend.identity, p.me.identity), sent, from),
      rs,
    ),
  );

  test('attach drains the inbox: two first messages from the fake friend', () {
    run((async, p, node) {
      final s = p.session;
      expect(s.connected, isTrue);
      expect(s.status!.ready, isTrue);
      expect(s.status!.booting, isTrue);
      expect(s.status!.inbox, 0);
      expect(s.received.map((r) => r.text), [
        'Hello from the fake node.',
        'A second message, waiting in the inbox.',
      ]);
      final f = friendOf(p, node);
      expect(f.name, 'Fake friend');
      expect(f.verified, isFalse);
      expect(s.received.map((r) => r.from), [f.key, f.key]);
      expect((p.sessionStore.state!['received'] as List).length, 2);
      expect(s.address.toString(), '${Address(FakeNode.gps, FakeNode.id)}');
    });
  });

  test('attach gives the node the phone\'s clock', () {
    run((async, p, node) {
      final t = p.session.nodeTime!;
      expect(t.source, TimeSource.phone);
      expect(t.utcS, start.millisecondsSinceEpoch ~/ 1000);
      expect(t.offsetMin, start.timeZoneOffset.inMinutes);
      expect(describeTime(t), startsWith('2026-11-05T00:00:00.000Z'));
    });
  });

  test('the first rotation counts down here', () {
    run((async, p, node) {
      final s = p.session;
      expect(s.bootLeftS, 195);
      async.elapse(const Duration(seconds: 100));
      expect(s.bootLeftS, inInclusiveRange(95, 96));
      async.elapse(const Duration(seconds: 100));
      expect(s.status!.booting, isFalse);
      expect(s.status!.relays, FakeNode.relaysHeard);
    });
  });

  test('the queue: taken, given back, not ready, offered again', () {
    run((async, p, node) {
      final s = p.session;
      final f = friendOf(p, node);
      for (final t in ['a', 'b', 'c', 'd']) {
        p.messenger.sendText(f, t);
        async.flushMicrotasks();
      }
      async.elapse(const Duration(seconds: 1));
      final [a, b, c, d] = s.outbox;
      expect(a.state, OutState.taken);
      expect(b.state, OutState.taken);
      expect(c.state, OutState.givenBack);
      expect(s.waitLeftS(c), 26);
      expect(d.state, OutState.queued);
      expect(s.status!.ready, isFalse);

      async.elapse(const Duration(seconds: 28));
      expect(s.waitLeftS(c), 0);
      expect(d.state, OutState.taken);

      s.offerAgain(c);
      async.elapse(const Duration(seconds: 1));
      expect(c.state, OutState.taken);

      async.elapse(const Duration(seconds: 10));
      expect(s.received.map((r) => r.text).where((t) => t.startsWith('Echo')), [
        'Echo: a',
        'Echo: b',
        'Echo: d',
        'Echo: c',
      ]);
      // Its echo was boxed: only the fake friend could make it.
      expect(friendOf(p, node).verified, isTrue);
    });
  });

  test('a text to someone who has written is boxed from the first', () {
    run((async, p, node) {
      // The fake friend's first messages show they hold the person's card.
      final f = friendOf(p, node);
      expect(f.verified, isFalse);
      expect(f.hasMine, isTrue);
      p.messenger.sendText(f, 'first');
      async.elapse(FakeNode.echoAfter + const Duration(seconds: 1));
      p.messenger.sendText(friendOf(p, node), 'second');
      async.flushMicrotasks();
      final [first, second] = p.session.outbox;
      expect(formOf(first.payload), Form.boxed);
      expect(formOf(second.payload), Form.boxed);
      expect(second.sent, greaterThan(first.sent));
      // The first names the node the person writes from; the echo answered
      // it, so the second does not.
      final (h1, _) = readPlain(openBox(friendPair(p, node), first.payload));
      expect(h1.address.toString(), p.me.nodeAddress.toString());
      final (h2, _) = readPlain(openBox(friendPair(p, node), second.payload));
      expect(h2.address, isNull);
      expect(h2.greater, greater(p.me.identity, node.friend.identity));
    });
  });

  test('the header names the node written from, not the home on the card', () {
    run((async, p, node) {
      p.messenger.sendText(friendOf(p, node), 'first');
      async.elapse(FakeNode.echoAfter + const Duration(seconds: 1));
      final home = Address(Position.fromDegrees(50.5, 30.5), List.filled(6, 9));
      p.me.setHome(home);
      p.me.setNodeAddress(elsewhere);
      async.flushMicrotasks();
      expect(p.me.card.home.toString(), home.toString());
      p.messenger.sendText(friendOf(p, node), 'moved');
      async.flushMicrotasks();
      final (h, _) = readPlain(
        openBox(friendPair(p, node), p.session.outbox.last.payload),
      );
      expect(h.address.toString(), elsewhere.toString());
      // Until it is answered, it is named again.
      p.messenger.sendText(friendOf(p, node), 'again');
      async.flushMicrotasks();
      final (h2, _) = readPlain(
        openBox(friendPair(p, node), p.session.outbox.last.payload),
      );
      expect(h2.address.toString(), elsewhere.toString());
    });
  });

  test(
    'a contact\'s two addresses are kept apart, the newest message wins',
    () {
      run((async, p, node) {
        final card = friendOf(p, node).card.home.toString();
        final there = Address(Position.fromDegrees(50.3, 30.4), [
          7,
          7,
          7,
          7,
          7,
          7,
        ]);
        final s0 = sentOf(start.add(async.elapsed)) + 10;
        void from(int sent, Address? a) {
          p.messenger.receive(
            InboxMessage(
              sent,
              fromFriend(node, p, sent, [const TextRecord('x')], a),
            ),
          );
          async.flushMicrotasks();
        }

        from(s0, there);
        var f = friendOf(p, node);
        expect(f.card.home.toString(), card);
        expect(f.wroteFrom.toString(), there.toString());
        expect(f.heardSent, s0);
        // Without an address: unchanged, and heard later.
        from(s0 + 5, null);
        f = friendOf(p, node);
        expect(f.wroteFrom.toString(), there.toString());
        expect(f.heardSent, s0 + 5);
        // An older message, as out of a mailbox, changes nothing.
        from(s0 + 1, elsewhere);
        f = friendOf(p, node);
        expect(f.wroteFrom.toString(), there.toString());
        expect(f.heardSent, s0 + 5);
        expect(f.card.home.toString(), card);
      });
    },
  );

  test('the fake friend: a reply goes where they were, then they choose', () {
    run((async, p, node) {
      var f = friendOf(p, node);
      expect(f.card.home.toString(), FakeNode.friendHome.toString());
      final first = f.wroteFrom.toString();
      expect(first, isNot(FakeNode.friendHome.toString()));
      expect(f.towards(start.add(async.elapsed)), [Toward.there]);

      p.messenger.sendText(f, 'hi');
      async.flushMicrotasks();
      final o = p.session.outbox.last;
      expect(o.toward, Toward.there);
      expect(o.dst.toString(), first);
      // The echo comes from a node that has moved on.
      async.elapse(FakeNode.echoAfter + const Duration(seconds: 1));
      f = friendOf(p, node);
      expect(f.wroteFrom.toString(), isNot(first));
      expect(f.wroteFrom.toString(), node.friendAway.toString());

      async.elapse(Contact.fresh);
      f = friendOf(p, node);
      expect(f.towards(start.add(async.elapsed)), [Toward.there, Toward.home]);
      p.messenger.sendText(f, 'home', toward: Toward.home);
      async.flushMicrotasks();
      expect(
        p.session.outbox.last.dst.toString(),
        FakeNode.friendHome.toString(),
      );
    });
  });

  test('a contact is heard at its sent, or on arrival if that is ahead', () {
    run((async, p, node) {
      async.elapse(const Duration(hours: 1));
      final now = start.add(async.elapsed);
      // Written ten minutes ago, fetched only now.
      final past = sentOf(now) - 600;
      p.messenger.receive(
        InboxMessage(1, fromFriend(node, p, past, [const TextRecord('a')])),
      );
      async.flushMicrotasks();
      expect(friendOf(p, node).heardAt, timeOfSent(past));
      // Their clock is a minute ahead of this phone's.
      p.messenger.receive(
        InboxMessage(
          2,
          fromFriend(node, p, sentOf(now) + 60, [const TextRecord('b')]),
        ),
      );
      async.flushMicrotasks();
      expect(friendOf(p, node).heardAt, now);
    });
  });

  test('an acknowledgement marks the text it names, and one is sent', () {
    run((async, p, node) {
      final f = friendOf(p, node);
      p.messenger.sendText(f, 'a');
      async.elapse(const Duration(seconds: 6));
      final a = p.session.outbox.single;
      expect(a.acked, isFalse);
      final ack = fromFriend(node, p, 99, [AckRecord(a.sent)]);
      p.messenger.receive(InboxMessage(1, ack));
      async.flushMicrotasks();
      expect(a.acked, isTrue);

      final echo = p.session.received.last;
      p.messenger.acknowledge(echo);
      async.elapse(const Duration(seconds: 1));
      expect(echo.acked, isTrue);
      final sent = p.session.outbox.last;
      expect(sent.kind, OutKind.ack);
      expect(formOf(sent.payload), Form.boxed);
    });
  });

  group('long texts', () {
    /// The records of a payload from the phone to the fake friend.
    (Header, List<Record>) opened(Phone p, FakeNode node, Uint8List payload) =>
        readPlain(switch (formOf(payload)) {
          Form.introduction => openIntro(sodium, node.friend, payload),
          _ => openBox(friendPair(p, node), payload),
        });

    test('go as parts of consecutive sent, queued in order', () {
      run((async, p, node) {
        final f = friendOf(p, node);
        final text = List.filled(60, 'word').join(' ');
        final parts = p.messenger.partsOf(f, text);
        expect(parts.length, greaterThan(1));
        p.messenger.sendText(f, text);
        async.flushMicrotasks();
        final out = p.session.outbox;
        expect(out.length, parts.length);
        final n = parts.length;
        for (final (i, o) in out.indexed) {
          expect((o.part, o.of, o.sent), (i + 1, n, out.first.sent + i));
          final (h, rs) = opened(p, node, o.payload);
          expect(h.sent, o.sent);
          final t = rs.whereType<TextRecord>().single;
          expect((t.text, t.part, t.of), (parts[i], i + 1, n));
          expect(o.payload.length, lessThanOrEqualTo(maxAppPayload));
        }
        expect(out.map((o) => o.body).join(), text);
        // The next message's sent is past the last part's.
        p.messenger.sendText(f, 'after');
        async.flushMicrotasks();
        expect(p.session.outbox.last.sent, out[n - 1].sent + 1);
      });
    });

    test('over 16 parts is refused, and nothing is queued', () {
      run((async, p, node) {
        final f = friendOf(p, node);
        Object? error;
        p.messenger.sendText(f, 'x' * 3000).catchError((e) => error = e);
        async.flushMicrotasks();
        expect(error, isA<BadPayload>());
        expect('$error', contains('16 at most'));
        expect(p.session.outbox, isEmpty);
      });
    });

    test('come together in any order; a tap acknowledges what has come', () {
      run((async, p, node) {
        void part(int id, int sent, String t, int n) => p.messenger.receive(
          InboxMessage(id, fromFriend(node, p, sent, [TextRecord(t, n, 3)])),
        );
        part(11, 5002, 'three', 3);
        part(12, 5000, 'one ', 1);
        async.flushMicrotasks();
        final s = p.session;
        final last = s.received.last;
        expect(s.partsOf(last).map((r) => r?.body), ['one ', null, 'three']);
        Entry entry() => thread(
          hex(node.friend.identity),
          p.contacts.list,
          s.outbox,
          s.received,
        ).entries.last;
        expect(entry().text, 'one \npart 2 of 3 not yet here\nthree');
        expect(entry().pieces.map((x) => x.missing), [false, true, false]);

        // One tap: one message naming both parts that came.
        p.messenger.acknowledge(last);
        async.flushMicrotasks();
        final ack = s.outbox.last;
        expect(ack.kind, OutKind.ack);
        expect(
          opened(p, node, ack.payload).$2.map((r) => (r as AckRecord).sent),
          [5000, 5002],
        );
        expect(entry().arrived.every((r) => r.acked), isTrue);

        // The part that comes later is the next tap's, alone.
        part(13, 5001, 'two ', 2);
        async.flushMicrotasks();
        expect(entry().text, 'one two three');
        expect(entry().arrived.every((r) => r.acked), isFalse);
        p.messenger.acknowledge(s.received.last);
        async.flushMicrotasks();
        expect(
          opened(
            p,
            node,
            s.outbox.last.payload,
          ).$2.map((r) => (r as AckRecord).sent),
          [5001],
        );
        // Its copy, read again, is not kept twice.
        part(14, 5001, 'two ', 2);
        async.flushMicrotasks();
        expect(s.received.where((r) => r.sent == 5001).length, 1);
      });
    });

    test('a sent text is acknowledged when every part is', () {
      run((async, p, node) {
        final f = friendOf(p, node);
        p.messenger.sendText(f, List.filled(60, 'word').join(' '));
        async.flushMicrotasks();
        final out = [...p.session.outbox];
        p.messenger.receive(
          InboxMessage(
            21,
            fromFriend(node, p, 9000, [
              for (final o in out.skip(1)) AckRecord(o.sent),
            ]),
          ),
        );
        async.flushMicrotasks();
        expect(out.map((o) => o.acked), [
          false,
          ...out.skip(1).map((_) => true),
        ]);
      });
    });
  });

  test('a message of one\'s own sent back does not open as the other\'s', () {
    run((async, p, node) {
      final f = friendOf(p, node);
      p.messenger.sendText(f, 'x');
      async.elapse(const Duration(seconds: 6));
      p.messenger.sendText(friendOf(p, node), 'mine');
      async.flushMicrotasks();
      final mine = p.session.outbox.last.payload;
      final before = p.session.received.length;
      p.messenger.receive(InboxMessage(2, mine));
      async.flushMicrotasks();
      expect(p.session.received.length, before + 1);
      expect(p.session.received.last.from, isNull);
    });
  });

  test('a first message naming a verified contact is marked as one', () {
    run((async, p, node) {
      final f = friendOf(p, node);
      p.messenger.sendText(f, 'x');
      async.elapse(const Duration(seconds: 6));
      expect(friendOf(p, node).verified, isTrue);
      final forged = sealIntro(
        sodium,
        p.me.identity,
        0,
        writePlain(Header(greater(node.friend.identity, p.me.identity), 500), [
          CardRecord(node.friendCard.encode()),
          const TextRecord('I am your friend, honest'),
        ]),
      );
      p.messenger.receive(InboxMessage(3, forged));
      async.flushMicrotasks();
      final r = p.session.received.last;
      expect(r.body, 'I am your friend, honest');
      expect(r.intro, isTrue);
      expect(friendOf(p, node).verified, isTrue);
    });
  });

  test('the mailbox: a backlog of 70 comes in batches, newest first', () {
    run((async, p, node) {
      final home = Address(Position.fromDegrees(50.5, 30.5), [
        9,
        9,
        9,
        9,
        9,
        9,
      ]);
      p.me.setHome(home);
      // The home's ring holds items 1 to 70, none sent; it answers as
      // its mailbox does, and the replies come in any order.
      final news = <String?>[];
      var lastSent = 0;
      for (final (top, bottom) in [(70, 39), (38, 7), (6, 1)]) {
        p.messenger.fetch(elsewhere);
        async.flushMicrotasks();
        final o = p.session.outbox.last;
        expect(o.kind, OutKind.fetch);
        expect(o.dst.toString(), home.toString());
        final f = Fetch.open(sodium, p.me.keys.mailbox, o.payload);
        expect(f.address.toString(), elsewhere.toString());
        // Each a later round, so that the home takes it.
        expect(f.sent, greaterThan(lastSent));
        lastSent = f.sent;
        for (var n = bottom; n <= top; n++) {
          final inner = fromFriend(node, p, 1000 + n, [TextRecord('t$n')]);
          final reply = sealReply(
            sodium,
            p.me.keys.mailbox,
            f.recipient,
            f.sent,
            top - n + 1,
            n,
            bottom - 1,
            inner,
          );
          p.messenger.receive(InboxMessage(n, reply));
          async.flushMicrotasks();
        }
        expect(o.acked, isTrue);
        expect(p.me.remaining, bottom - 1);
        news.add(p.session.fetchNews);
        p.session.fetchNews = null;
      }
      expect(news, [
        '38 more at home: fetch again for them',
        '6 more at home: fetch again for them',
        null,
      ]);
      final got = p.session.received.where((r) => r.mailbox).toList();
      expect(got.length, 70);
      // The same item again, read at home after its reply, is not kept
      // twice.
      p.messenger.receive(
        InboxMessage(300, fromFriend(node, p, 1006, [TextRecord('t6')])),
      );
      async.flushMicrotasks();
      expect(p.session.received.where((r) => r.body == 't6').length, 1);
      // A reply under another key opens for nobody.
      final other = Keys.fromSeed(sodium, List.filled(16, 7));
      final stray = sealReply(sodium, other.mailbox, 0, lastSent, 4, 74, 0, [
        1,
      ]);
      p.messenger.receive(InboxMessage(200, stray));
      async.flushMicrotasks();
      expect(p.session.received.last.from, isNull);
    });
  });

  test('given back while away is read at the next connect', () {
    run((async, p, node) {
      final s = p.session;
      final f = friendOf(p, node);
      for (final t in ['a', 'b', 'c']) {
        p.messenger.sendText(f, t);
        async.flushMicrotasks();
      }
      async.elapse(const Duration(milliseconds: 400));
      expect(s.outbox[2].state, OutState.taken);
      s.disconnect();
      async.elapse(const Duration(seconds: 10));
      expect(s.connected, isFalse);
      s.attach(node.connect());
      async.flushMicrotasks();
      expect(s.outbox[2].state, OutState.givenBack);
      expect(s.waitLeftS(s.outbox[2]), 16);
    });
  });

  test('the log and the last given back', () {
    run((async, p, node) {
      final s = p.session;
      final f = friendOf(p, node);
      for (final t in ['a', 'b', 'c']) {
        p.messenger.sendText(f, t);
        async.flushMicrotasks();
      }
      async.elapse(const Duration(seconds: 1));
      final c = s.outbox[2];
      expect(s.givenBack!.msgId, c.id);
      final lines = s.log.map((l) => l.text).toList();
      expect(lines.first, startsWith('Node: bark-fa4e00000001'));
      expect(lines, contains('Send ${msgIdHex(c.id)}: taken'));
      expect(lines, contains(startsWith('Given back: ${msgIdHex(c.id)}')));
      expect(lines, contains(startsWith('Inbox: confirmed')));
    });
  });

  test('a node at link version 2 is attached and given the clock', () {
    fakeAsync((async) {
      final s = Session(store: MemoryStore(), now: () => start);
      final node = Version2Node(
        now: () => start,
        sodium: sodium,
        app: List.filled(32, 1),
      );
      bool? ok;
      s.attach(node.connect()).then((v) => ok = v);
      async.flushMicrotasks();
      expect(ok, isTrue);
      expect(s.node!.version, 2);
      expect(s.nodeTime!.source, TimeSource.phone);
      node.dispose();
      s.dispose();
    });
  });

  test('a node with a newer link is refused', () {
    fakeAsync((async) {
      final s = Session(store: MemoryStore(), now: () => start);
      final node = NewerNode(
        now: () => start,
        sodium: sodium,
        app: List.filled(32, 1),
      );
      bool? ok;
      s.attach(node.connect()).then((v) => ok = v);
      async.flushMicrotasks();
      expect(ok, isFalse);
      expect(s.connected, isFalse);
      expect(s.notice, contains('newer'));
      node.dispose();
    });
  });

  test('the identity, contacts, queue and texts survive a restart', () {
    run((async, p, node) {
      p.messenger.sendText(friendOf(p, node), 'kept');
      async.flushMicrotasks();
      final again = p.again(sodium, () => start);
      async.flushMicrotasks();
      expect(again.me.key, p.me.key);
      expect(again.me.lastSent, p.me.lastSent);
      expect(again.contacts.list.single.key, hex(node.friend.identity));
      expect(again.session.received.length, 2);
      expect(again.session.received.first.from, hex(node.friend.identity));
      expect(again.session.outbox.single.text, 'kept');
      expect(again.session.outbox.single.id, p.session.outbox.single.id);
    });
  });

  test('the node paired with, its loss and a stop survive a restart', () {
    run((async, p, node) {
      p.session.setPaired('bark-fa4e00000001');
      async.flushMicrotasks();
      var again = p.again(sodium, () => start);
      async.flushMicrotasks();
      expect(again.session.pairedWith, 'bark-fa4e00000001');
      expect(again.session.pairingLost, isFalse);
      again.session.setPaired('bark-fa4e00000001', lost: true);
      async.flushMicrotasks();
      again = again.again(sodium, () => start);
      async.flushMicrotasks();
      expect(again.session.pairingLost, isTrue);
      expect(again.session.stopped, isFalse);
      again.session.setStopped(true);
      async.flushMicrotasks();
      again = again.again(sodium, () => start);
      async.flushMicrotasks();
      expect(again.session.stopped, isTrue);
    });
  });

  test('a fetch in flight is still waited for after a restart', () {
    run((async, p, node) {
      p.me.setHome(
        Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]),
      );
      p.messenger.fetch(elsewhere);
      async.flushMicrotasks();
      final o = p.session.fetchWaiting!;
      final again = p.again(sodium, () => start.add(async.elapsed));
      async.flushMicrotasks();
      expect(again.session.fetchWaiting?.id, o.id);
      // Its answer, nothing new, ends the wait there too.
      again.messenger.receive(
        InboxMessage(
          1,
          sealNothingNew(sodium, again.me.keys.mailbox, 0, o.sent),
        ),
      );
      async.flushMicrotasks();
      expect(again.session.fetchWaiting, isNull);
      expect(again.session.fetchNews, 'No new mail');
    });
  });

  test('the fake home answers a fetch with its item, then nothing new', () {
    fakeAsync((async) {
      DateTime now() => start.add(async.elapsed);
      final p = Phone(sodium, now);
      async.flushMicrotasks();
      final node = FakeNode(
        now: now,
        sodium: sodium,
        app: p.me.identity,
        mailbox: p.me.keys.mailbox,
      );
      // The replies come back to this node though the fetch names another,
      // whose tag a public node would not take for the owner's.
      node.public = false;
      p.session.attach(node.connect());
      async.flushMicrotasks();
      p.me.setHome(
        Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]),
      );
      for (final first in [true, false]) {
        p.messenger.fetch(elsewhere);
        async.elapse(FakeNode.fetchAnswerAfter + FakeNode.handOver);
        expect(p.session.fetchWaiting, isNull);
        expect(p.session.fetchNews, first ? isNull : 'No new mail');
        expect(p.me.remaining, 0);
      }
      expect(
        p.session.received
            .where((r) => r.mailbox || r.body != null)
            .map((r) => r.body),
        contains('Left at your home node while you were away.'),
      );
      node.dispose();
      p.session.dispose();
    });
  });

  test('sent numbers rise even when the clock goes back', () {
    fakeAsync((async) {
      var t = start;
      final me = Me(MemoryStore(), sodium, () => t);
      me.load();
      async.flushMicrotasks();
      late int a, b;
      me.nextSent().then((v) => a = v);
      async.flushMicrotasks();
      t = t.subtract(const Duration(hours: 1));
      me.nextSent().then((v) => b = v);
      async.flushMicrotasks();
      expect(a, sentOf(start));
      expect(b, a + 1);
    });
  });

  test('the fake answers a short Send with invalid length', () {
    fakeAsync((async) {
      final node = FakeNode(
        now: () => start,
        sodium: sodium,
        app: List.filled(32, 1),
      );
      Object? err;
      node.connect().send(elsewhere.encode().sublist(0, 12)).catchError((e) {
        err = e;
      });
      async.flushMicrotasks();
      expect((err as AttException).code, AttError.invalidLength);
      node.dispose();
    });
  });

  test('a card read from its text, and what is not one', () {
    final k = Keys.fromSeed(sodium, List.filled(16, 1));
    for (final (name, home) in [
      ('', elsewhere),
      ('Olena', null),
      ('Олена Ш.', elsewhere),
      ('🐕 bark', null),
    ]) {
      final c = PersonCard(k.identity, home, 7, name);
      final back = PersonCard.parse(c.toText());
      expect(back.name, name);
      expect(back.key, c.key);
      expect(back.home?.toString(), home?.toString());
      expect(back.time, 7);
    }
    // A home flag without room for the address.
    final short = PersonCard(k.identity, null, 7, 'abc').encode()..[1] = 1;
    expect(() => PersonCard.decode(short), throwsA(isA<BadCard>()));
    // A card of version 1, before the flags, and an unknown flag.
    final b = PersonCard(k.identity, elsewhere, 7, '').encode();
    for (final (at, v, why) in [(0, 1, 'older'), (1, 2, 'Not a card')]) {
      final bad = Uint8List.fromList(b)..[at] = v;
      expect(
        () => PersonCard.decode(bad),
        throwsA(predicate((e) => e is BadCard && '$e'.contains(why))),
      );
    }
    for (final t in ['', 'BARK:', 'hello', 'BARK:A1', elsewhere.toString()]) {
      expect(() => PersonCard.parse(t), throwsA(isA<BadCard>()), reason: t);
    }
    expect(
      () => PersonCard(k.identity, null, 0, 'x' * 33).encode(),
      throwsA(isA<BadCard>()),
    );
  });
}
