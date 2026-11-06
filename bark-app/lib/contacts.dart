// The person's contacts, kept on the phone, each known by its identity.

import 'package:flutter/foundation.dart';

import 'card.dart';
import 'link.dart';
import 'store.dart';

/// A card with the name this phone gives it.
class Contact {
  final PersonCard card;
  final String name;

  /// A boxed message from this identity has opened here: only its
  /// holder could make one. A first message's sender is not verified.
  final bool verified;

  /// The address of the person's own card this contact has been seen to
  /// know, and the one last sent to it.
  final String? knowsMine;
  final String? sentMine;

  Contact(
    this.card,
    this.name, {
    this.verified = false,
    this.knowsMine,
    this.sentMine,
  });

  Address get address => card.address;

  String get key => card.key;

  Contact copyWith({
    PersonCard? card,
    String? name,
    bool? verified,
    String? knowsMine,
    String? sentMine,
  }) => Contact(
    card ?? this.card,
    name ?? this.name,
    verified: verified ?? this.verified,
    knowsMine: knowsMine ?? this.knowsMine,
    sentMine: sentMine ?? this.sentMine,
  );

  Map<String, dynamic> toJson() => {
    'card': card.toText(),
    'name': name,
    'verified': verified,
    'knowsMine': knowsMine,
    'sentMine': sentMine,
  };

  static Contact fromJson(Map<String, dynamic> j) => Contact(
    PersonCard.parse(j['card'] as String),
    j['name'] as String,
    verified: j['verified'] as bool,
    knowsMine: j['knowsMine'] as String?,
    sentMine: j['sentMine'] as String?,
  );
}

class Contacts extends ChangeNotifier {
  final Store store;
  Contacts(this.store);

  final List<Contact> list = [];

  Contact? byKey(String? key) => list.where((c) => c.key == key).firstOrNull;

  /// The version of `contacts.json`'s format.
  static const version = 1;

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
