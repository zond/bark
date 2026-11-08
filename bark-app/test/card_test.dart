// The card is shown only once the person has a name; a scanned card
// without one, as an older app may send, still makes a contact, listed
// by the start of its identity until a name comes.

import 'dart:convert';

import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
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
}
