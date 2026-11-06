// Where a text to a contact may go: where they last wrote from while that
// is fresh, else the person chooses, or the one address that stands.

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final now = DateTime.utc(2026, 11, 6, 12);
  final cardAt = Address(Position.fromDegrees(50.45, 30.52), List.filled(6, 1));
  final there = Address(Position.fromDegrees(50.40, 30.60), List.filled(6, 2));

  Contact contact({required bool home, int cardTime = 0}) => Contact(
    PersonCard(List.filled(32, 5), cardAt, cardTime, 'Olena', home: home),
    'Olena',
  );

  /// Heard from at `there`, `age` ago.
  Contact wrote(Contact c, Duration age) {
    final at = now.subtract(age);
    return c.heard(sentOf(at), there, at);
  }

  test('nothing known: the card\'s address, no choice', () {
    expect(contact(home: false).towards(now), [Toward.card]);
    expect(contact(home: true).towards(now), [Toward.home]);
  });

  test('where they wrote from, while fresh, wins', () {
    for (final home in [true, false]) {
      final c = wrote(contact(home: home), const Duration(seconds: 599));
      expect(c.towards(now), [Toward.there]);
      expect(c.addressOf(Toward.there).toString(), there.toString());
    }
  });

  test('stale, with a home: both, for the person to choose', () {
    final c = wrote(contact(home: true), Contact.fresh);
    expect(c.towards(now), [Toward.there, Toward.home]);
    expect(c.unasked(now), Toward.home);
    expect(c.addressOf(Toward.home).toString(), cardAt.toString());
  });

  test('stale, without a home: the newer of the two, no choice', () {
    final c = wrote(contact(home: false), const Duration(hours: 2));
    expect(c.towards(now), [Toward.there]);
    // A card whose address became theirs after they last wrote.
    final later = sentOf(now.subtract(const Duration(hours: 1)));
    final d = wrote(
      contact(home: false, cardTime: later),
      const Duration(hours: 2),
    );
    expect(d.towards(now), [Toward.card]);
  });

  test('written from the card\'s own address: the card\'s', () {
    final at = now.subtract(const Duration(minutes: 1));
    final c = contact(home: true).heard(sentOf(at), cardAt, at);
    expect(c.towards(now), [Toward.home]);
  });
}
