// The person's contacts, kept on the phone, each known by its identity.

import 'package:flutter/foundation.dart';

import 'app_layer.dart';
import 'card.dart';
import 'link.dart';
import 'session.dart';
import 'store.dart';

/// A card with the name this phone gives it.
class Contact {
  final PersonCard card;
  final String name;

  /// A boxed message from this identity has opened here: only its
  /// holder could make one. A first message's sender is not verified.
  final bool verified;

  /// The address the person writes from that this contact has been seen
  /// to know, and the one last sent to it.
  final String? knowsMine;
  final String? sentMine;

  /// The address this contact's messages were last written from, from
  /// their headers; kept apart from the card's, which may be a home's.
  final Address? wroteFrom;

  /// The `sent` of the newest message from them, and when it was written
  /// by this phone's clock: no later than it arrived.
  final int? heardSent;
  final DateTime? heardAt;

  Contact(
    this.card,
    this.name, {
    this.verified = false,
    this.knowsMine,
    this.sentMine,
    this.wroteFrom,
    this.heardSent,
    this.heardAt,
  });

  /// The delivery TTL, the age at which the spec's "Delivery queue" calls
  /// a message stale: until then the network keeps mail for a node at the
  /// address it last used.
  static const fresh = Duration(seconds: 600);

  /// Where a text to them may go at `now`: where they last wrote from
  /// while that is fresh. Past it, that or a home's mailbox, for the
  /// person to choose; without a home the card's address is only where
  /// their node was too, and the newer of the two stands alone.
  List<Toward> towards(DateTime now) {
    final cards = card.home ? Toward.home : Toward.card;
    final from = wroteFrom, at = heardAt, sent = heardSent;
    if (from == null || at == null || sent == null) return [cards];
    if (from.toString() == card.address.toString()) return [cards];
    if (now.difference(at) < fresh) return [Toward.there];
    if (card.home) return [Toward.there, Toward.home];
    return [sent >= card.time ? Toward.there : Toward.card];
  }

  /// Where a message the person does not place goes: the only choice, or
  /// the home's mailbox, which keeps it.
  Toward unasked(DateTime now) => towards(now).last;

  Address addressOf(Toward t) => t == Toward.there ? wroteFrom! : card.address;

  String get key => card.key;

  Contact copyWith({
    PersonCard? card,
    String? name,
    bool? verified,
    String? knowsMine,
    String? sentMine,
    Address? wroteFrom,
    int? heardSent,
    DateTime? heardAt,
  }) => Contact(
    card ?? this.card,
    name ?? this.name,
    verified: verified ?? this.verified,
    knowsMine: knowsMine ?? this.knowsMine,
    sentMine: sentMine ?? this.sentMine,
    wroteFrom: wroteFrom ?? this.wroteFrom,
    heardSent: heardSent ?? this.heardSent,
    heardAt: heardAt ?? this.heardAt,
  );

  /// After a message from them, written at `sent` with `address` in its
  /// header, that arrived at `arrived`: only the newest counts, and one
  /// without an address says it has not changed.
  Contact heard(int sent, Address? address, DateTime arrived) {
    final last = heardSent;
    if (last != null && sent <= last) return this;
    final written = timeOfSent(sent);
    return copyWith(
      wroteFrom: address,
      heardSent: sent,
      heardAt: written.isAfter(arrived) ? arrived : written,
    );
  }

  Map<String, dynamic> toJson() => {
    'card': card.toText(),
    'name': name,
    'verified': verified,
    'knowsMine': knowsMine,
    'sentMine': sentMine,
    'wroteFrom': wroteFrom?.toString(),
    'heardSent': heardSent,
    'heardAt': heardAt?.toIso8601String(),
  };

  static Contact fromJson(Map<String, dynamic> j) => Contact(
    PersonCard.parse(j['card'] as String),
    j['name'] as String,
    verified: j['verified'] as bool,
    knowsMine: j['knowsMine'] as String?,
    sentMine: j['sentMine'] as String?,
    wroteFrom: j['wroteFrom'] == null
        ? null
        : Address.parse(j['wroteFrom'] as String),
    heardSent: j['heardSent'] as int?,
    heardAt: j['heardAt'] == null
        ? null
        : DateTime.parse(j['heardAt'] as String),
  );
}

class Contacts extends ChangeNotifier {
  final Store store;
  Contacts(this.store);

  final List<Contact> list = [];

  Contact? byKey(String? key) => list.where((c) => c.key == key).firstOrNull;

  /// The version of `contacts.json`'s format: 2 since the cards it holds
  /// carry the home flag, card version 2.
  static const version = 2;

  Future<void> load() async {
    final l = await store.load(
      version,
      (j) => [
        for (final e in j['contacts'] as List)
          Contact.fromJson(e as Map<String, dynamic>),
      ],
    );
    if (l == null) return;
    list.addAll(l);
    notifyListeners();
  }

  Future<void> _save() => store.save(version, {
    'contacts': [for (final c in list) c.toJson()],
  });

  /// Adds a contact, or replaces the one with the same identity.
  Future<void> put(Contact c) async {
    final i = list.indexWhere((o) => o.key == c.key);
    if (i < 0) {
      list.add(c);
    } else {
      list[i] = c;
    }
    list.sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    notifyListeners();
    await _save();
  }

  Future<void> remove(Contact c) async {
    list.removeWhere((o) => o.key == c.key);
    notifyListeners();
    await _save();
  }
}
