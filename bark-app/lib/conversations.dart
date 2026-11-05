// Sent and received texts grouped by the other person's node, for the
// conversation list and the threads.

import 'contacts.dart';
import 'link.dart';
import 'session.dart';

/// One text in a thread: sent or received.
class Entry {
  final Outgoing? sent;
  final Received? received;
  Entry.sent(Outgoing this.sent) : received = null;
  Entry.received(Received this.received) : sent = null;

  DateTime get at => sent?.at ?? received!.at;
  String get text => sent?.text ?? received!.text;
}

class Conversation {
  /// The other node's ID in hex, or null for texts whose sender the app
  /// cannot tell.
  final String? key;
  final Contact? contact;

  /// Where a reply goes: the contact's address, else the latest one
  /// seen in the texts; null when the sender is unknown.
  final Address? address;

  /// Oldest first.
  final List<Entry> entries;

  Conversation(this.key, this.contact, this.address, this.entries);

  String get title => contact?.name ?? address?.toString() ?? 'Sender unknown';

  Entry? get last => entries.lastOrNull;
}

/// The conversations, newest first. A contact without texts has none.
List<Conversation> conversations(
  List<Contact> contacts,
  List<Outgoing> outbox,
  List<Received> received,
) {
  // Ties keep sent before received and each list's own order, which
  // List.sort, not being stable, would not.
  final all = [
    for (final (i, o) in outbox.indexed) (Entry.sent(o), o.dst, 0, i),
    for (final (i, r) in received.indexed) (Entry.received(r), r.from, 1, i),
  ];
  all.sort((x, y) {
    final t = x.$1.at.compareTo(y.$1.at);
    if (t != 0) return t;
    return x.$3 != y.$3 ? x.$3 - y.$3 : x.$4 - y.$4;
  });
  final byKey = <String?, List<Entry>>{};
  final seen = <String, Address>{};
  for (final (e, a, _, _) in all) {
    final k = a == null ? null : nodeKey(a);
    (byKey[k] ??= []).add(e);
    if (a != null) seen[k!] = a;
  }
  final list = [
    for (final MapEntry(key: k, value: es) in byKey.entries)
      _conversation(k, contacts, seen[k], es),
  ];
  list.sort((x, y) => y.last!.at.compareTo(x.last!.at));
  return list;
}

Conversation _conversation(
  String? key,
  List<Contact> contacts,
  Address? seen,
  List<Entry> entries,
) {
  final c = key == null
      ? null
      : contacts.where((c) => c.key == key).firstOrNull;
  return Conversation(key, c, c?.address ?? seen, entries);
}

/// The thread with one node, empty if no text has gone either way.
Conversation thread(
  String? key,
  List<Contact> contacts,
  List<Outgoing> outbox,
  List<Received> received,
) =>
    conversations(
      contacts,
      outbox,
      received,
    ).where((c) => c.key == key).firstOrNull ??
    _conversation(key, contacts, null, []);
