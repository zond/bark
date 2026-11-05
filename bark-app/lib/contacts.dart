// The person's contacts and their own name, kept on the phone.

import 'package:flutter/foundation.dart';

import 'card.dart';
import 'link.dart';
import 'store.dart';

/// What a contact and a conversation are known by: the node ID in hex.
String nodeKey(Address a) => hex(a.id);

/// A card with the name this phone gives it.
class Contact {
  final PersonCard card;
  final String name;
  Contact(this.card, this.name);

  Address get address => card.address;

  /// A contact is its node: a card scanned again after the node moved
  /// has a new position but the same node ID.
  String get key => nodeKey(address);

  Map<String, dynamic> toJson() => {'card': card.toText(), 'name': name};

  static Contact fromJson(Map<String, dynamic> j) =>
      Contact(PersonCard.parse(j['card'] as String), j['name'] as String);
}

class Contacts extends ChangeNotifier {
  final Store store;
  Contacts(this.store);

  final List<Contact> list = [];

  /// The name the person puts on their card.
  String myName = '';

  /// The node's address as last read, so the card shows while the node
  /// is away.
  Address? myAddress;

  Contact? byKey(String key) => list.where((c) => c.key == key).firstOrNull;

  Future<void> load() async {
    final j = await store.load();
    if (j == null) return;
    list.addAll(
      (j['contacts'] as List).map(
        (e) => Contact.fromJson(e as Map<String, dynamic>),
      ),
    );
    myName = j['myName'] as String;
    final a = j['myAddress'] as String?;
    myAddress = a == null ? null : Address.parse(a);
    notifyListeners();
  }

  Future<void> _save() => store.save({
    'contacts': [for (final c in list) c.toJson()],
    'myName': myName,
    'myAddress': myAddress?.toString(),
  });

  Future<void> _changed() async {
    notifyListeners();
    await _save();
  }

  /// Adds a contact, or replaces the one for the same node.
  Future<void> put(Contact c) async {
    final i = list.indexWhere((o) => o.key == c.key);
    if (i < 0) {
      list.add(c);
    } else {
      list[i] = c;
    }
    list.sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    await _changed();
  }

  Future<void> remove(Contact c) async {
    list.removeWhere((o) => o.key == c.key);
    await _changed();
  }

  Future<void> setMyName(String name) async {
    myName = name;
    await _changed();
  }

  Future<void> setMyAddress(Address a) async {
    if (a.toString() == myAddress?.toString()) return;
    myAddress = a;
    await _changed();
  }
}
