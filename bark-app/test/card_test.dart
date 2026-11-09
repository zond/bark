// The card is shown only once the person has a name; a scanned card
// without one, as an older app may send, still makes a contact, listed
// by the start of its identity until a name comes; and an archived
// contact's card scanned again brings it back. A card carries a home and
// the node it was made at, reads a card of version 2, and without a home
// is temporary until a later card brings one.

import 'dart:convert';

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/store.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

void main() {
  test('the card is shown only with a name', () async {
    final sodium = await SodiumInit.init();
    final me = Me(MemoryStore(), sodium, DateTime.now);
    await me.load();
    expect(me.shownCard, isNull);
    await me.setName('  ');
    expect(me.shownCard, isNull);
    await me.setName('Ana');
    expect(me.shownCard!.name, 'Ana');
    expect(me.shownCard!.key, me.key);
  });

  test('a nameless card: listed as Unnamed until a name comes', () async {
    final cs = Contacts(MemoryStore());
    final id = List.filled(32, 0xab);
    await cs.add(PersonCard(id, null, 1, ''));
    expect(cs.list.single.name, 'Unnamed (abababab…)');
    expect(cs.list.single.given, '');

    // An older card changes nothing; a newer one's name shows.
    await cs.add(PersonCard(id, null, 0, 'Old'));
    expect(cs.list.single.name, 'Unnamed (abababab…)');
    await cs.add(PersonCard(id, null, 2, 'Olena'));
    expect(cs.list.single.name, 'Olena');

    // A name the person gives stays over the card's.
    await cs.put(cs.list.single.copyWith(name: 'Aunt'));
    await cs.add(PersonCard(id, null, 3, 'Olena K'));
    expect(cs.list.single.name, 'Aunt');
    expect(cs.list.single.card.name, 'Olena K');
  });

  test('scanning an archived contact\'s card brings it back', () async {
    final cs = Contacts(MemoryStore());
    final card = PersonCard(List.filled(32, 0xab), null, 1, 'Olena');
    await cs.archiveContact(await cs.add(card));
    expect(cs.active, isEmpty);
    expect(cs.isArchived(card.key), isTrue);
    final back = await cs.add(card);
    expect(back.archived, isFalse);
    expect(cs.active.single.key, card.key);
    expect(cs.isArchived(card.key), isFalse);
  });

  test('a contact saved as "Unnamed" before reads as nameless', () async {
    final store = MemoryStore();
    final card = PersonCard(List.filled(32, 0xab), null, 1, '');
    store.text = jsonEncode({
      'version': Contacts.version,
      'contacts': [
        {
          'card': card.toText(),
          'name': 'Unnamed',
          'verified': false,
          'knowsMine': null,
          'sentMine': null,
          'wroteFrom': null,
          'heardSent': null,
          'heardAt': null,
        },
      ],
    });
    final cs = Contacts(store);
    await cs.load();
    expect(cs.list.single.chosen, '');
    expect(cs.list.single.name, 'Unnamed (abababab…)');
  });

  final home = Address(Position.fromDegrees(50.45, 30.52), List.filled(6, 1));
  final node = Address(Position.fromDegrees(50.44, 30.50), List.filled(6, 3));

  test('a card carries its home, and the node with its link time', () {
    final id = List.filled(32, 0xab);
    for (final (h, n) in [(home, node), (null, node), (home, null)]) {
      final c = PersonCard.parse(
        PersonCard(id, h, 7, 'Olena', now: n, nowAt: 99).toText(),
      );
      expect(c.home?.toString(), h?.toString());
      expect(c.now?.toString(), n?.toString());
      expect(c.nowAt, n == null ? 0 : 99);
      expect(c.time, 7);
      expect(c.name, 'Olena');
      expect(c.temporary, h == null);
    }
    final b = PersonCard(
      id,
      home,
      7,
      'Olena',
      now: node,
      nowAt: 0x01020304,
    ).encode();
    expect(b[0], 3);
    expect(b[1], 3);
    expect(b.length, PersonCard.fixedLen + 2 * Address.len + 4 + 5);
    final at = PersonCard.fixedLen + 2 * Address.len;
    expect(b.sublist(at, at + 4), [1, 2, 3, 4]);
    // A node without room for its time is not a card.
    expect(
      () => PersonCard.decode(b.sublist(0, at + 2)),
      throwsA(isA<BadCard>()),
    );
  });

  test('my card: the node and, while linked, now; after, the link\'s end; '
      'none when the node is the home', () async {
    var t = DateTime.utc(2026, 11, 9, 12, 0, 30);
    final me = Me(MemoryStore(), await SodiumInit.init(), () => t);
    await me.load();
    await me.setNodeAddress(node);
    expect(me.card.now.toString(), node.toString());
    // To the minute, so the code does not change every second.
    expect(me.card.nowAt, sentOf(DateTime.utc(2026, 11, 9, 12)));
    t = t.add(const Duration(minutes: 10));
    expect(me.card.nowAt, sentOf(DateTime.utc(2026, 11, 9, 12, 10)));
    await me.unlinked();
    t = t.add(const Duration(hours: 1));
    expect(me.card.nowAt, sentOf(DateTime.utc(2026, 11, 9, 12, 10, 30)));
    await me.setHome(node);
    expect(me.card.now, isNull);
  });

  test('a card of version 2 is still read', () {
    final id = List.filled(32, 0xab);
    final v3 = PersonCard(id, home, 7, 'Olena').encode();
    final v2 = [2, ...v3.sublist(1)];
    final c = PersonCard.decode(v2);
    expect(c.home.toString(), home.toString());
    expect(c.now, isNull);
    expect(c.name, 'Olena');
    // Version 2 knew no node flag.
    expect(
      () => PersonCard.decode([2, 2, ...v3.sublist(2)]),
      throwsA(isA<BadCard>()),
    );
  });

  test(
    'a temporary contact loses its mark with a later card with a home',
    () async {
      final me = Me(MemoryStore(), await SodiumInit.init(), DateTime.now);
      await me.load();
      await me.setName('Ana');
      await me.setNodeAddress(node);
      expect(me.card.temporary, isTrue);
      expect(me.card.now.toString(), node.toString());
      final cs = Contacts(MemoryStore());
      await cs.add(me.card);
      expect(cs.list.single.card.temporary, isTrue);
      await me.setHome(home);
      await cs.add(me.card);
      expect(cs.list.single.card.temporary, isFalse);
    },
  );
}
