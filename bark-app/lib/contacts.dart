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

  /// A message of theirs, in either form, has opened here: it was sealed
  /// to the person's identity, so they hold the person's card, and a box
  /// to them opens. Until then a message to them carries the card.
  bool get hasMine => verified || heardSent != null;

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

  /// Put away by the person, with its conversation: kept, so that its
  /// messages still open, but out of the contacts list.
  final bool archived;

  Contact(
    this.card,
    this.name, {
    this.verified = false,
    this.knowsMine,
    this.sentMine,
    this.wroteFrom,
    this.heardSent,
    this.heardAt,
    this.archived = false,
  });

  /// How long where they wrote from is taken as where they are: in a
  /// minute a person on foot, at a brisk 1.5 m/s, moves about 90 m, well
  /// within the 750 m a relay hears a handheld in the street (spec
  /// "Physical layer"), while one driving has already left that reach, so
  /// a longer wait would not make the guess any better.
  static const fresh = Duration(seconds: 60);

  /// Where a text to them may go at `now`: where they last wrote from
  /// while that is fresh; past it, that or their home's mailbox, for the
  /// person to choose. None when they have no home and have not written.
  List<Toward> towards(DateTime now) {
    final from = wroteFrom, at = heardAt, home = card.home;
    if (from == null || at == null) return [if (home != null) Toward.home];
    if (home == null) return [Toward.there];
    if (from.toString() == home.toString()) return [Toward.home];
    if (now.difference(at) < fresh) return [Toward.there];
    return [Toward.there, Toward.home];
  }

  /// Where a message the person does not place goes: the only choice, or
  /// the home's mailbox, which keeps it; null when there is none.
  Toward? unasked(DateTime now) => towards(now).lastOrNull;

  Address addressOf(Toward t) => t == Toward.there ? wroteFrom! : card.home!;

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
    bool? archived,
  }) => Contact(
    card ?? this.card,
    name ?? this.name,
    verified: verified ?? this.verified,
    knowsMine: knowsMine ?? this.knowsMine,
    sentMine: sentMine ?? this.sentMine,
    wroteFrom: wroteFrom ?? this.wroteFrom,
    heardSent: heardSent ?? this.heardSent,
    heardAt: heardAt ?? this.heardAt,
    archived: archived ?? this.archived,
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
    'archived': archived,
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
    // Absent in a file from before archiving: nothing was archived.
    archived: j['archived'] as bool? ?? false,
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

  /// The version of `contacts.json`'s format: 2 since the cards it holds
  /// are of card version 2.
  static const version = 2;

  Future<void> load() async {
    final l = await store.load(
      version,
      (j) => (
        [
          for (final e in j['contacts'] as List)
            Contact.fromJson(e as Map<String, dynamic>),
        ],
        // Absent in a file from before archiving: nothing was archived.
        [for (final k in j['archived'] as List? ?? []) k as String],
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

  Future<void> remove(Contact c) async {
    list.removeWhere((o) => o.key == c.key);
    notifyListeners();
    await _save();
  }
}
