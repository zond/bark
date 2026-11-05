// Grouping sent and received texts into conversations.

import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/conversations.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/payload.dart';
import 'package:bark_app/session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final t0 = DateTime.utc(2026, 11, 5, 12);
  DateTime at(int min) => t0.add(Duration(minutes: min));
  Address addr(int n, {double lat = 50}) =>
      Address(Position.fromDegrees(lat, 30), [0, 0, 0, 0, 0, n]);
  final olena = Contact(PersonCard(addr(1), 'O.'), 'Olena');
  Outgoing out(Address to, String t, int min) =>
      Outgoing(to, encodeText(t), at: at(min));
  Received got(Address? from, String t, int min) =>
      Received(0, encodeText(t), at(min), from: from);

  test('one conversation a node, newest first, texts in order', () {
    final cs = conversations(
      [olena],
      [out(addr(1), 'hi', 0), out(addr(2), 'to a stranger', 2)],
      [got(addr(1), 'hello', 1), got(addr(1), 'how are you', 3)],
    );
    expect(cs.map((c) => c.title), ['Olena', addr(2).toString()]);
    expect(cs.first.entries.map((e) => e.text), ['hi', 'hello', 'how are you']);
    expect(cs.first.entries.map((e) => e.sent != null), [true, false, false]);
    expect(cs.first.contact, same(olena));
  });

  test('a contact is its node, wherever the node has moved', () {
    final moved = addr(1, lat: 51);
    final cs = conversations([olena], [out(addr(1), 'before', 0)], [
      got(moved, 'after', 1),
    ]);
    expect(cs.single.title, 'Olena');
    expect(cs.single.entries.length, 2);
    // A reply goes to the address on the contact's card.
    expect(cs.single.address.toString(), addr(1).toString());
  });

  test('a stranger is named by the latest address, until named', () {
    final moved = addr(2, lat: 51);
    final texts = [got(addr(2), 'one', 0), got(moved, 'two', 1)];
    final c = conversations([olena], [], texts).single;
    expect(c.contact, isNull);
    expect(c.title, moved.toString());
    expect(c.address.toString(), moved.toString());
    final named = Contact(PersonCard(moved, ''), 'Petro');
    expect(conversations([olena, named], [], texts).single.title, 'Petro');
  });

  test('texts whose sender is unknown have their own conversation', () {
    final cs = conversations([olena], [out(addr(1), 'hi', 0)], [
      got(null, 'who?', 1),
    ]);
    expect(cs.first.key, isNull);
    expect(cs.first.title, 'Sender unknown');
    expect(cs.first.address, isNull);
  });

  test('at the same time, sent before received, each in its order', () {
    final c = conversations(
      [olena],
      [out(addr(1), 'a', 0), out(addr(1), 'b', 0)],
      [got(addr(1), 'c', 0), got(addr(1), 'd', 0)],
    ).single;
    expect(c.entries.map((e) => e.text), ['a', 'b', 'c', 'd']);
  });

  test('the thread with a contact who has no texts is empty', () {
    final c = thread(olena.key, [olena], [], []);
    expect(c.entries, isEmpty);
    expect(c.title, 'Olena');
    expect(c.address.toString(), addr(1).toString());
  });
}
