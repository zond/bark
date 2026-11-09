// Where a text to a contact may go: where they last wrote from while that
// is fresh, else the person chooses between that and their home's
// mailbox, or the one address there is; without a home and before they
// write, the node on their card.

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

  Contact contact(Address? home, {Address? at}) => Contact(
    PersonCard(List.filled(32, 5), home, 0, 'Olena', now: at),
    'Olena',
  );

  /// Heard from at `there`, `age` ago.
  Contact wrote(Contact c, Duration age) {
    final at = now.subtract(age);
    return c.heard(sentOf(at), there, at);
  }

  test('nothing written from: the home, else the node on the card', () {
    expect(contact(home, at: node).towards(now), [Toward.home]);
    expect(contact(home).addressOf(Toward.home).toString(), home.toString());
    final temporary = contact(null, at: node);
    expect(temporary.towards(now), [Toward.node]);
    expect(temporary.addressOf(Toward.node).toString(), node.toString());
    // Only a card from before D162 names no node.
    expect(contact(null).towards(now), isEmpty);
    expect(contact(null).unasked(now), isNull);
  });

  test('without a home, where they wrote from over the card\'s node', () {
    final c = wrote(contact(null, at: node), const Duration(hours: 2));
    expect(c.towards(now), [Toward.there]);
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
