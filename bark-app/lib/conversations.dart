// Sent and received texts grouped by the other person's identity, for
// the conversation list and the threads.

import 'contacts.dart';
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
  /// The other person's identity in hex.
  final String key;
  final Contact? contact;

  /// Oldest first.
  final List<Entry> entries;

  Conversation(this.key, this.contact, this.entries);

  String get title => contact?.name ?? 'Removed contact';

  Entry? get last => entries.lastOrNull;
}

/// The conversations, newest first. A contact without texts has none;
/// a payload that opened for nobody is in none.
List<Conversation> conversations(
  List<Contact> contacts,
  List<Outgoing> outbox,
  List<Received> received,
) {
  // Ties keep sent before received and each list's own order, which
  // List.sort, not being stable, would not.
  final all = [
    for (final (i, o) in outbox.indexed)
      if (o.kind == OutKind.text && o.to != null) (Entry.sent(o), o.to!, 0, i),
    for (final (i, r) in received.indexed)
      if (r.from != null && r.body != null) (Entry.received(r), r.from!, 1, i),
  ];
  all.sort((x, y) {
    final t = x.$1.at.compareTo(y.$1.at);
    if (t != 0) return t;
    return x.$3 != y.$3 ? x.$3 - y.$3 : x.$4 - y.$4;
  });
  final byKey = <String, List<Entry>>{};
  for (final (e, k, _, _) in all) {
    (byKey[k] ??= []).add(e);
  }
  final list = [
    for (final MapEntry(key: k, value: es) in byKey.entries)
      Conversation(k, contacts.where((c) => c.key == k).firstOrNull, es),
  ];
  list.sort((x, y) => y.last!.at.compareTo(x.last!.at));
  return list;
}

/// The thread with one identity, empty if no text has gone either way.
Conversation thread(
  String key,
  List<Contact> contacts,
  List<Outgoing> outbox,
  List<Received> received,
) =>
    conversations(
      contacts,
      outbox,
      received,
    ).where((c) => c.key == key).firstOrNull ??
    Conversation(key, contacts.where((c) => c.key == key).firstOrNull, []);
