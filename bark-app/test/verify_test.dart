// Two phones writing to each other with no node between them: which form
// each message takes and when each side verifies the other; a first
// message that a box overtook, and one forged after it, both kept as
// texts marked as first messages that change nothing else.

import 'dart:typed_data';

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/messenger.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// One person's phone, its messages carried by hand.
class Person {
  final Me me;
  final Contacts contacts;
  final Session session;
  late final Messenger messenger;

  /// How many of the outbox's messages have been carried.
  int carried = 0;

  Person(Sodium s, DateTime Function() now)
    : me = Me(MemoryStore(), s, now),
      contacts = Contacts(MemoryStore()),
      session = Session(store: MemoryStore(), now: now) {
    messenger = Messenger(s, me, contacts, session, now);
    me.load();
    contacts.load();
    session.load();
  }

  /// Once loaded: a name, and a home node that is also where they write
  /// from.
  void settle(String name, Address home) {
    me.setName(name);
    me.setHome(home);
    me.setNodeAddress(home);
  }

  /// Adds `other`'s card, as a scan does.
  void scan(Person other, String name) =>
      contacts.put(Contact(other.me.card, name));

  Contact of(Person other) => contacts.byKey(other.me.key)!;
}

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 6);
  Address at(int n) => Address(Position.fromDegrees(50.0 + n / 100, 30.0), [
    0xba,
    0x4c,
    0,
    0,
    0,
    n,
  ]);

  void run(void Function(FakeAsync, Person, Person) body) => fakeAsync((async) {
    DateTime now() => start.add(async.elapsed);
    final a = Person(sodium, now);
    final b = Person(sodium, now);
    async.flushMicrotasks();
    a.settle('Anna', at(1));
    b.settle('Bohdan', at(2));
    async.flushMicrotasks();
    body(async, a, b);
  });

  /// Writes `text` from `from` to `to` and returns its form.
  Form write(FakeAsync async, Person from, Person to, String text) {
    from.messenger.sendText(from.of(to), text);
    async.flushMicrotasks();
    async.elapse(const Duration(seconds: 2));
    return formOf(from.session.outbox.last.payload)!;
  }

  /// Hands `from`'s outbox message `i` to `to`'s inbox.
  void deliver(FakeAsync async, Person from, Person to, int i) {
    to.messenger.receive(InboxMessage(i, from.session.outbox[i].payload));
    async.flushMicrotasks();
  }

  /// Hands everything `from` has queued to `to`'s inbox, but `held`,
  /// the outbox's indices of messages that take longer.
  void carry(
    FakeAsync async,
    Person from,
    Person to, {
    Set<int> held = const {},
  }) {
    final o = from.session.outbox;
    for (; from.carried < o.length; from.carried++) {
      if (held.contains(from.carried)) continue;
      deliver(async, from, to, from.carried);
    }
  }

  List<String?> texts(Person p) => [for (final r in p.session.received) r.body];

  /// Both have each other's card, have read a message of the other's and
  /// write boxes; one more message each way verifies both.
  void endsVerified(FakeAsync async, Person a, Person b) {
    expect(write(async, a, b, 'a again'), Form.boxed);
    expect(write(async, b, a, 'b again'), Form.boxed);
    carry(async, a, b);
    carry(async, b, a);
    expect(a.of(b).verified, isTrue);
    expect(b.of(a).verified, isTrue);
    expect(texts(a).last, 'b again');
    expect(texts(b).last, 'a again');
  }

  for (final first in ['A', 'B']) {
    test('$first scans the other and writes first', () {
      run((async, a, b) {
        final (w, r) = first == 'A' ? (a, b) : (b, a);
        w.scan(r, 'them');
        async.flushMicrotasks();
        expect(write(async, w, r, 'hello'), Form.introduction);
        carry(async, w, r);
        // The reader learns the writer's card from the introduction, but
        // anyone holding the reader's card could have made it.
        expect(r.of(w).verified, isFalse);
        expect(r.of(w).hasMine, isTrue);
        expect(texts(r), ['hello']);
        // The introduction proves the writer holds the reader's card: the
        // answer is boxed, and verifies the reader at the writer.
        expect(write(async, r, w, 'hi'), Form.boxed);
        carry(async, r, w);
        expect(w.of(r).verified, isTrue);
        expect(texts(w), ['hi']);
        // The writer's next is boxed, and verifies the writer.
        expect(write(async, w, r, 'how are you'), Form.boxed);
        carry(async, w, r);
        expect(r.of(w).verified, isTrue);
        endsVerified(async, a, b);
      });
    });
  }

  test('both scan and both write before either reads', () {
    run((async, a, b) {
      a.scan(b, 'B');
      b.scan(a, 'A');
      async.flushMicrotasks();
      expect(write(async, a, b, 'from a'), Form.introduction);
      expect(write(async, b, a, 'from b'), Form.introduction);
      carry(async, a, b);
      carry(async, b, a);
      expect(a.of(b).verified, isFalse);
      expect(b.of(a).verified, isFalse);
      expect(texts(a), ['from b']);
      expect(texts(b), ['from a']);
      endsVerified(async, a, b);
    });
  });

  test('a first message from an unknown identity stays unverified', () {
    run((async, a, b) {
      a.scan(b, 'B');
      async.flushMicrotasks();
      write(async, a, b, 'one');
      write(async, a, b, 'two');
      carry(async, a, b);
      final c = b.of(a);
      expect(c.name, 'Anna');
      expect(c.verified, isFalse);
      expect(texts(b), ['one', 'two']);
      // Nothing of B's has opened at A: A still introduces itself.
      expect(a.of(b).hasMine, isFalse);
      expect(write(async, a, b, 'three'), Form.introduction);
    });
  });

  test('a boxed message from an unverified contact verifies it', () {
    run((async, a, b) {
      a.scan(b, 'B');
      b.scan(a, 'A');
      async.flushMicrotasks();
      // As a phone that already had a message from B would write.
      a.contacts.put(a.of(b).copyWith(heardSent: 1));
      async.flushMicrotasks();
      expect(write(async, a, b, 'boxed'), Form.boxed);
      expect(b.of(a).verified, isFalse);
      carry(async, a, b);
      expect(b.of(a).verified, isTrue);
      expect(texts(b), ['boxed']);
    });
  });

  /// A first message, A's to B, that B reads only after a box of A's:
  /// A writes two first messages, B reads the first and answers, A's
  /// next is boxed, and B reads it before the second.
  void overtaken(FakeAsync async, Person a, Person b) {
    a.scan(b, 'B');
    async.flushMicrotasks();
    expect(write(async, a, b, 'one'), Form.introduction);
    expect(write(async, a, b, 'two'), Form.introduction);
    carry(async, a, b, held: {1});
    expect(write(async, b, a, 'hi'), Form.boxed);
    carry(async, b, a);
    expect(write(async, a, b, 'three'), Form.boxed);
    carry(async, a, b);
    expect(b.of(a).verified, isTrue);
  }

  bool intro(Person p, String text) =>
      p.session.received.singleWhere((r) => r.body == text).intro;

  test('a first message overtaken by a box is kept, marked', () {
    run((async, a, b) {
      overtaken(async, a, b);
      final heard = b.of(a).heardSent;
      deliver(async, a, b, 1);
      expect(texts(b), ['one', 'three', 'two']);
      expect(intro(b, 'one'), isTrue);
      expect(intro(b, 'two'), isTrue);
      expect(intro(b, 'three'), isFalse);
      expect(b.of(a).verified, isTrue);
      expect(b.of(a).heardSent, heard);
    });
  });

  test('a first message forged after a box is kept, marked, and changes '
      'nothing else', () {
    run((async, a, b) {
      overtaken(async, a, b);
      // B's text, without a reaction.
      write(async, b, a, 'unanswered');
      final mine = b.session.outbox.last;
      final before = b.of(a);
      // Anyone holding both cards: a new address, and a reaction to a
      // text A has not read, each after the card.
      Uint8List forged(int sent, Record r) => sealIntro(
        sodium,
        b.me.identity,
        0,
        writePlain(Header(greater(a.me.identity, b.me.identity), sent, at(9)), [
          CardRecord(a.me.card.encode()),
          r,
        ]),
      );
      b.messenger.receive(
        InboxMessage(98, forged(1 << 30, ReactionRecord(mine.sent, '👍'))),
      );
      b.messenger.receive(
        InboxMessage(99, forged((1 << 30) + 1, const TextRecord('forged'))),
      );
      async.flushMicrotasks();
      expect(texts(b).last, 'forged');
      expect(intro(b, 'forged'), isTrue);
      final after = b.of(a);
      expect(after.verified, isTrue);
      expect(after.place.toString(), before.place.toString());
      expect(after.heardSent, before.heardSent);
      expect(mine.reaction, isNull);
    });
  });
}
