// Archiving: a conversation alone, or a contact with its conversation;
// either brought back with the other; kept across a restart; and a file
// from before archiving read as nothing archived.

import 'dart:convert';

import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/store.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  Contact named(int n, String name) =>
      Contact(PersonCard(List.filled(32, n), null, 0, name), name);
  final olena = named(1, 'Olena'), petro = named(2, 'Petro');
  final stranger = hex(List.filled(32, 3));

  Future<Contacts> two(Store s) async {
    final cs = Contacts(s);
    await cs.load();
    await cs.put(olena);
    await cs.put(petro);
    return cs;
  }

  test('a conversation archived alone keeps its contact', () async {
    final cs = await two(MemoryStore());
    await cs.archiveConversation(olena.key);
    expect(cs.isArchived(olena.key), isTrue);
    expect(cs.active.map((c) => c.name), ['Olena', 'Petro']);
    // A removed contact's conversation is archived the same way.
    await cs.archiveConversation(stranger);
    expect(cs.isArchived(stranger), isTrue);
    await cs.unarchive(olena.key);
    expect(cs.isArchived(olena.key), isFalse);
  });

  test('a contact archived takes its conversation; either brings both '
      'back', () async {
    final cs = await two(MemoryStore());
    await cs.archiveContact(olena);
    expect(cs.isArchived(olena.key), isTrue);
    expect(cs.active.map((c) => c.name), ['Petro']);
    // Still known, so its messages open.
    expect(cs.byKey(olena.key)!.archived, isTrue);
    await cs.unarchive(olena.key);
    expect(cs.isArchived(olena.key), isFalse);
    expect(cs.byKey(olena.key)!.archived, isFalse);
    // Unarchiving where nothing is archived changes nothing.
    await cs.unarchive(petro.key);
    expect(cs.active.length, 2);
  });

  test('kept across a restart', () async {
    final s = MemoryStore('contacts.json');
    final cs = await two(s);
    await cs.archiveContact(petro);
    await cs.archiveConversation(stranger);
    final again = Contacts(s);
    await again.load();
    expect(again.archived, {petro.key, stranger});
    expect(again.byKey(petro.key)!.archived, isTrue);
    expect(again.byKey(olena.key)!.archived, isFalse);
  });

  test(
    'a contacts.json from before archiving reads as nothing archived',
    () async {
      final c = olena.toJson()..remove('archived');
      final old = jsonEncode({
        'version': Contacts.version,
        'contacts': [c],
      });
      final s = MemoryStore('contacts.json')..text = old;
      final cs = Contacts(s);
      await cs.load();
      expect(s.fault, isNull);
      expect(cs.archived, isEmpty);
      expect(cs.active.map((c) => c.name), ['Olena']);
    },
  );
}
