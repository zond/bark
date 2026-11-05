// The card's bytes and its text form, which other phones will read.

import 'dart:convert';

import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/store.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final kyiv = Position.fromDegrees(50.45, 30.52);
  final a = Address(kyiv, [1, 2, 3, 4, 5, 6]);

  test('a card is a version, the address and the name', () {
    final c = PersonCard(a, 'Olena');
    expect(c.encode(), [
      1, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 1, 2, 3, 4, 5, 6, //
      ...utf8.encode('Olena'),
    ]);
    expect(c.toText(), 'AUfASQrZ_4ABAgMEBQZPbGVuYQ==');
  });

  test('a card round-trips, with any name or none', () {
    for (final name in ['', 'Olena', 'Олена Ш.', '🐕 bark']) {
      final c = PersonCard.parse(PersonCard(a, name).toText());
      expect(c.address.toString(), a.toString());
      expect(c.name, name);
    }
  });

  test('what is not a card is refused, and says why', () {
    expect(() => PersonCard.decode([]), throwsA(isA<BadCard>()));
    expect(() => PersonCard.decode([1, 2, 3]), throwsA(isA<BadCard>()));
    expect(
      () => PersonCard.decode([0, ...a.encode()]),
      throwsA(isA<BadCard>()),
    );
    expect(
      () => PersonCard.decode([1, ...a.encode(), 0xff]),
      throwsA(isA<BadCard>()),
    );
    expect(
      () => PersonCard.parse('https://example.org/'),
      throwsA(isA<BadCard>()),
    );
    expect(
      () => PersonCard.decode([2, ...a.encode()]),
      throwsA(
        isA<BadCard>().having((e) => e.why, 'why', contains('newer app')),
      ),
    );
  });

  test('contacts and my name survive a restart; one contact a node', () async {
    final store = MemoryStore();
    final cs = Contacts(store);
    await cs.put(Contact(PersonCard(a, 'Olena'), 'Olena'));
    final moved = Address(Position.fromDegrees(50.0, 30.0), a.id);
    await cs.put(Contact(PersonCard(moved, 'Olena'), 'Aunt Olena'));
    await cs.setMyName('Taras');
    await cs.setMyAddress(a);
    final again = Contacts(store);
    await again.load();
    expect(again.list.single.name, 'Aunt Olena');
    expect(again.list.single.address.toString(), moved.toString());
    expect(again.myName, 'Taras');
    expect(again.myAddress.toString(), a.toString());
  });
}
