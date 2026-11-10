// Where a text to a contact may go: the one place they were last known
// at, from their messages or their card, the newer winning, while that is
// fresh; else the person chooses between that and their home's mailbox,
// or the one address there is.

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final now = DateTime.utc(2026, 11, 6, 12);
  final home = Address(Position.fromDegrees(50.45, 30.52), List.filled(6, 1));
  final there = Address(Position.fromDegrees(50.40, 30.60), List.filled(6, 2));

  final node = Address(Position.fromDegrees(50.44, 30.50), List.filled(6, 3));

  /// Olena's contact from her card, which names `at` as linked `linked`
  /// ago.
  Contact contact(Address? home, {Address? at, Duration? linked}) {
    final c = PersonCard(
      List.filled(32, 5),
      home,
      0,
      'Olena',
      now: at,
      nowAt: sentOf(now.subtract(linked ?? const Duration(hours: 1))),
    );
    return Contact(c, 'Olena').withCard(c);
  }

  /// Heard from at `there`, `age` ago.
  Contact wrote(Contact c, Duration age) {
    final at = now.subtract(age);
    return c.heard(sentOf(at), there, at);
  }

  test('nothing written from: the home, else the node on the card', () {
    expect(contact(home).towards(now), [Toward.home]);
    expect(contact(home).addressOf(Toward.home).toString(), home.toString());
    final temporary = contact(null, at: node);
    expect(temporary.towards(now), [Toward.there]);
    expect(temporary.addressOf(Toward.there).toString(), node.toString());
    expect(temporary.stale(now), const Duration(hours: 1));
    // A card made before its phone used a node names none.
    expect(contact(null).towards(now), isEmpty);
    expect(contact(null).unasked(now), isNull);
  });

  test('one last known place: the newer of a message and a card wins', () {
    // A message newer than the card's link time moves the place.
    var c = wrote(contact(null, at: node), const Duration(minutes: 30));
    expect(c.place.toString(), there.toString());
    expect(c.placeAt, now.subtract(const Duration(minutes: 30)));
    // An older card does not move it back; a newer one does.
    final card = c.card;
    PersonCard linked(Duration ago) => PersonCard(
      card.identity,
      null,
      0,
      'Olena',
      now: node,
      nowAt: sentOf(now.subtract(ago)),
    );
    c = c.withCard(linked(const Duration(hours: 2)));
    expect(c.place.toString(), there.toString());
    c = c.withCard(linked(const Duration(minutes: 5)));
    expect(c.place.toString(), node.toString());
    expect(c.placeAt, now.subtract(const Duration(minutes: 5)));
    // A message older than the card's link time does not move it.
    c = wrote(c, const Duration(minutes: 10));
    expect(c.place.toString(), node.toString());
    // A newer message without an address: still there, known later.
    final later = now.subtract(const Duration(minutes: 1));
    c = c.heard(sentOf(later), null, later);
    expect(c.place.toString(), node.toString());
    expect(c.placeAt, later);
  });

  test('a card with a home over a fresh place: there while fresh', () {
    final c = contact(home, at: node, linked: const Duration(seconds: 30));
    expect(c.towards(now), [Toward.there]);
    expect(
      contact(home, at: node, linked: const Duration(hours: 1)).towards(now),
      [Toward.there, Toward.home],
    );
  });

  test('where they wrote from, while fresh, wins', () {
    for (final h in [home, null]) {
      final c = wrote(contact(h), const Duration(seconds: 59));
      expect(c.towards(now), [Toward.there]);
      expect(c.addressOf(Toward.there).toString(), there.toString());
    }
  });

  test('stale, with a home: both, for the person to choose', () {
    final c = wrote(contact(home), Contact.fresh);
    expect(c.towards(now), [Toward.there, Toward.home]);
    expect(c.unasked(now), Toward.home);
  });

  test('stale, without a home: where they were, no choice', () {
    final c = wrote(contact(null), const Duration(hours: 2));
    expect(c.towards(now), [Toward.there]);
  });

  test('written from their home: the home', () {
    final at = now.subtract(const Duration(hours: 1));
    final c = contact(home).heard(sentOf(at), home, at);
    expect(c.towards(now), [Toward.home]);
  });
}
