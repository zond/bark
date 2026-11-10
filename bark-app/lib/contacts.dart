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

  /// The name the person gave this contact; empty for none, when the
  /// card's own name stands, so that a later card's name shows.
  final String chosen;

  /// The name the person gave, or else the card's; empty when neither
  /// has one.
  String get given => chosen.isNotEmpty ? chosen : card.name;

  /// What the contact is listed as: a nameless one by the start of its
  /// identity, so that two of them are told apart and no row is empty.
  String get name =>
      given.isNotEmpty ? given : 'Unnamed (${key.substring(0, 8)}…)';

  /// A boxed message from this identity has opened here: only its
  /// holder could make one. A first message's sender is not verified.
  final bool verified;

  /// A message of theirs, in either form, has opened here: it was sealed
  /// to the person's identity, so they hold the person's card, and a box
  /// to them opens. Until then a message to them carries the card.
  bool get hasMine => verified || heardSent != null;

  /// The address the person writes from that this contact has been seen
  /// to know, and the one last sent to it.
  final String? knowsMine;
  final String? sentMine;

  /// The node they were last known at, and when: the newer of their
  /// messages' origin and their card's node with its link time (roadmap
  /// D162); kept apart from the card's home.
  final Address? place;
  final DateTime? placeAt;

  /// The `sent` of the newest message from them, and when it was written
  /// by this phone's clock: no later than it arrived.
  final int? heardSent;
  final DateTime? heardAt;

  /// Put away by the person, with its conversation: kept, so that its
  /// messages still open, but out of the contacts list.
  final bool archived;

  /// Made from a first message that came, not from a card the person
  /// scanned or pasted: until verified, anyone could have sent it.
  final bool cameByMessage;

  /// Whether to say it is not verified: only a contact that came by
  /// message, as a card taken from its holder's hand needs no proof.
  bool get unverified => cameByMessage && !verified;

  Contact(
    this.card,
    this.chosen, {
    this.verified = false,
    this.knowsMine,
    this.sentMine,
    this.place,
    this.placeAt,
    this.heardSent,
    this.heardAt,
    this.archived = false,
    this.cameByMessage = false,
  });

  /// How long where they wrote from is taken as where they are: in a
  /// minute a person on foot, at a brisk 1.5 m/s, moves about 90 m, well
  /// within the 750 m a relay hears a handheld in the street (spec
  /// "Physical layer"), while one driving has already left that reach, so
  /// a longer wait would not make the guess any better.
  static const fresh = Duration(seconds: 60);

  /// Where a text to them may go at `now`, by the one place they were
  /// last known at (roadmap D132, D162): there while that is fresh; past
  /// it, with a home, there or the home's mailbox, for the person to
  /// choose; without one, there, as nothing else is. With a home and no
  /// place, or the place the home, the home. None for a card without a
  /// home or a node, from one who has not written.
  List<Toward> towards(DateTime now) {
    final at = place, when = placeAt, home = card.home;
    if (at == null || when == null) return [if (home != null) Toward.home];
    if (home == null) return [Toward.there];
    if (at.toString() == home.toString()) return [Toward.home];
    if (stale(now) == null) return [Toward.there];
    return [Toward.there, Toward.home];
  }

  /// How long ago they were last known at their place, once that is past
  /// [fresh]; null while it is fresh or unknown.
  Duration? stale(DateTime now) {
    final when = placeAt;
    if (when == null) return null;
    final age = now.difference(when);
    return age < fresh ? null : age;
  }

  /// Where a message the person does not place goes: the only choice, or
  /// the home's mailbox, which keeps it; null when there is none.
  Toward? unasked(DateTime now) => towards(now).lastOrNull;

  Address addressOf(Toward t) => switch (t) {
    Toward.there => place!,
    Toward.home => card.home!,
  };

  /// With `c`, a card of theirs as new as the one held or newer: its node
  /// is their place if it is newer than the one known.
  Contact withCard(PersonCard c) {
    final n = c.now;
    final t = timeOfSent(c.nowAt);
    final newer = n != null && (placeAt == null || t.isAfter(placeAt!));
    return copyWith(
      card: c,
      place: newer ? n : null,
      placeAt: newer ? t : null,
    );
  }

  String get key => card.key;

  Contact copyWith({
    PersonCard? card,
    String? name,
    bool? verified,
    String? knowsMine,
    String? sentMine,
    Address? place,
    DateTime? placeAt,
    int? heardSent,
    DateTime? heardAt,
    bool? archived,
    bool? cameByMessage,
  }) => Contact(
    card ?? this.card,
    name ?? chosen,
    verified: verified ?? this.verified,
    knowsMine: knowsMine ?? this.knowsMine,
    sentMine: sentMine ?? this.sentMine,
    place: place ?? this.place,
    placeAt: placeAt ?? this.placeAt,
    heardSent: heardSent ?? this.heardSent,
    heardAt: heardAt ?? this.heardAt,
    archived: archived ?? this.archived,
    cameByMessage: cameByMessage ?? this.cameByMessage,
  );

  /// After a message from them, written at `sent` with `address` in its
  /// header, that arrived at `arrived`: only the newest counts, and one
  /// without an address says it has not changed.
  Contact heard(int sent, Address? address, DateTime arrived) {
    final last = heardSent;
    if (last != null && sent <= last) return this;
    final w = timeOfSent(sent);
    final written = w.isAfter(arrived) ? arrived : w;
    // A message without an address says they are still where they were;
    // an older one than the place known, from a newer card, changes it not.
    // Not before: of two messages, heardSent has put them in order.
    final newer = placeAt == null || !written.isBefore(placeAt!);
    return copyWith(
      place: newer ? address : null,
      placeAt: newer && (address != null || place != null) ? written : null,
      heardSent: sent,
      heardAt: written,
    );
  }

  Map<String, dynamic> toJson() => {
    'card': card.toText(),
    'name': chosen,
    'verified': verified,
    'knowsMine': knowsMine,
    'sentMine': sentMine,
    'place': place?.toString(),
    'placeAt': placeAt?.toIso8601String(),
    'heardSent': heardSent,
    'heardAt': heardAt?.toIso8601String(),
    'archived': archived,
    'cameByMessage': cameByMessage,
  };

  static Contact fromJson(Map<String, dynamic> j) => Contact(
    PersonCard.parse(j['card'] as String),
    j['name'] as String,
    verified: j['verified'] as bool,
    knowsMine: j['knowsMine'] as String?,
    sentMine: j['sentMine'] as String?,
    place: switch (j['place']) {
      null => null,
      final a => Address.parse(a as String),
    },
    placeAt: switch (j['placeAt']) {
      null => null,
      final t => DateTime.parse(t as String),
    },
    heardSent: j['heardSent'] as int?,
    heardAt: j['heardAt'] == null
        ? null
        : DateTime.parse(j['heardAt'] as String),
    archived: j['archived'] as bool,
    cameByMessage: j['cameByMessage'] as bool,
  );
}

class Contacts extends ChangeNotifier {
  final Store store;
  Contacts(this.store);

  final List<Contact> list = [];

  Contact? byKey(String? key) => list.where((c) => c.key == key).firstOrNull;

  /// The contacts the person has not archived.
  Iterable<Contact> get active => list.where((c) => !c.archived);

  /// The identities whose conversation is archived: a removed contact's
  /// too, so it is kept apart from the contacts. An archived contact's is
  /// always among them.
  final Set<String> archived = {};

  bool isArchived(String key) => archived.contains(key);

  /// Hides the conversation with `key` from the first screen.
  Future<void> archiveConversation(String key) async {
    if (!archived.add(key)) return;
    notifyListeners();
    await _save();
  }

  /// Hides `c` from the contacts and its conversation from the first
  /// screen.
  Future<void> archiveContact(Contact c) async {
    archived.add(c.key);
    await put(c.copyWith(archived: true));
  }

  /// Brings back the conversation with `key` and its contact: a contact
  /// whose conversation is back is one the person talks to again.
  Future<void> unarchive(String key) async {
    final c = byKey(key);
    final had = archived.remove(key);
    if (c != null && c.archived) {
      await put(c.copyWith(archived: false));
    } else if (had) {
      notifyListeners();
      await _save();
    }
  }

  /// The version of `contacts.json`'s format.
  static const version = 2;

  Future<void> load() async {
    final l = await store.load(
      version,
      (j) => (
        [
          for (final e in j['contacts'] as List)
            Contact.fromJson(e as Map<String, dynamic>),
        ],
        [for (final k in j['archived'] as List) k as String],
      ),
    );
    if (l == null) return;
    list.addAll(l.$1);
    archived.addAll(l.$2);
    notifyListeners();
  }

  Future<void> _save() => store.save(version, {
    'contacts': [for (final c in list) c.toJson()],
    'archived': [...archived],
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

  /// Adds the contact of a card scanned or pasted, named `name` if that
  /// is not empty; a known one keeps its name, and its card unless this
  /// one is as new or newer. An archived one comes back: adding it again
  /// is asking for it back.
  Future<Contact> add(PersonCard card, {String name = ''}) async {
    final known = byKey(card.key);
    final c =
        (known == null
                ? Contact(card, '').withCard(card)
                : card.time >= known.card.time
                ? known.withCard(card)
                : known)
            .copyWith(name: name.isEmpty ? null : name);
    await put(c);
    await unarchive(c.key);
    return byKey(c.key)!;
  }

  Future<void> remove(Contact c) async {
    list.removeWhere((o) => o.key == c.key);
    notifyListeners();
    await _save();
  }
}
