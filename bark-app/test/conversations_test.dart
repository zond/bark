// Grouping sent and received texts into conversations, one an identity.

import 'dart:typed_data';

import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/conversations.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final t0 = DateTime.utc(2026, 11, 5, 12);
  DateTime at(int min) => t0.add(Duration(minutes: min));
  final where = Address(Position.fromDegrees(50, 30), [0, 0, 0, 0, 0, 1]);
  String id(int n) => hex(List.filled(32, n));
  final olena = Contact(
    PersonCard(List.filled(32, 1), where, 0, 'O.'),
    'Olena',
  );
  final petro = Contact(PersonCard(List.filled(32, 2), where, 0, ''), 'Petro');
  var sent = 0;
  Outgoing out(String to, String t, int min, {OutKind kind = OutKind.text}) =>
      Outgoing(
        where,
        Uint8List.fromList([sent++]),
        at: at(min),
        kind: kind,
        sent: sent,
        to: to,
        body: t,
      );
  Received got(String? from, String t, int min) => Received(
    0,
    [sent++],
    at(min),
    from: from,
    sent: sent,
    body: from == null ? null : t,
  );

  test('one conversation an identity, newest first, texts in order', () {
    final cs = conversations(
      [olena, petro],
      [out(id(1), 'hi', 0), out(id(2), 'to Petro', 2)],
      [got(id(1), 'hello', 1), got(id(1), 'how are you', 3)],
    );
    expect(cs.map((c) => c.title), ['Olena', 'Petro']);
    expect(cs.first.entries.map((e) => e.text), ['hi', 'hello', 'how are you']);
    expect(cs.first.entries.map((e) => e.mine), [true, false, false]);
    expect(cs.first.contact, same(olena));
  });

  test('reactions, cards, fetches and what did not open are in none', () {
    final cs = conversations(
      [olena],
      [
        out(id(1), 'hi', 0),
        out(id(1), '', 1, kind: OutKind.reaction),
        out(id(1), '', 2, kind: OutKind.card),
      ],
      [got(null, 'noise', 3)],
    );
    expect(cs.single.entries.map((e) => e.text), ['hi']);
  });

  test('at the same time, sent before received, each in its order', () {
    final c = conversations([olena], [out(id(1), 'a', 0), out(id(1), 'b', 0)], [
      got(id(1), 'c', 0),
      got(id(1), 'd', 0),
    ]).single;
    expect(c.entries.map((e) => e.text), ['a', 'b', 'c', 'd']);
  });

  test('the thread with a contact who has no texts is empty', () {
    final c = thread(olena.key, [olena], [], []);
    expect(c.entries, isEmpty);
    expect(c.title, 'Olena');
  });

  test('a removed contact\'s texts stay, under a plain title', () {
    final c = conversations([], [out(id(1), 'hi', 0)], []).single;
    expect(c.contact, isNull);
    expect(c.title, 'Removed contact');
  });

  test('a text\'s parts are one entry, in order, as far as they came', () {
    Received part(int sent, int n, String t, int min) => Received(
      0,
      [sent],
      at(min),
      from: id(1),
      sent: sent,
      body: t,
      part: n,
      of: 3,
    );
    Outgoing outPart(int sent, int n, String t, int min) => Outgoing(
      where,
      Uint8List.fromList([sent]),
      at: at(min),
      kind: OutKind.text,
      sent: sent,
      to: id(1),
      body: t,
      part: n,
      of: 2,
    );
    // Received last part first; a whole text between; another text's
    // part of the same count after.
    final c = conversations(
      [olena],
      [outPart(100, 1, 'mine ', 0), outPart(101, 2, 'whole', 0)],
      [
        part(502, 3, 'c', 1),
        got(id(1), 'between', 2),
        part(500, 1, 'a ', 3),
        part(700, 1, 'next', 4),
      ],
    ).single;
    expect(c.entries.map((e) => e.text), [
      'mine whole',
      'a \npart 2 of 3 not yet here\nc',
      'between',
      'next\npart 2 of 3 not yet here\npart 3 of 3 not yet here',
    ]);
    // A text stands where its first part to come stands.
    expect(c.entries[1].at, at(1));
  });
}
