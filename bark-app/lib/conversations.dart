// Sent and received texts grouped by the other person's identity, for
// the conversation list and the threads.

import 'contacts.dart';
import 'session.dart';

/// One text in a thread, sent or received, of one part or several.
class Entry {
  /// A sent text's parts, in order; empty for a received one.
  final List<Outgoing> outs;

  /// A received text's parts by number, null where one has not come;
  /// empty for a sent one.
  final List<Received?> ins;

  Entry.sent(this.outs) : ins = const [];
  Entry.received(this.ins) : outs = const [];

  bool get mine => outs.isNotEmpty;

  /// The parts of a received text that have come.
  Iterable<Received> get arrived => ins.nonNulls;

  /// When the first part was queued or came.
  DateTime get at => mine
      ? outs.first.at
      : arrived.map((r) => r.at).reduce((a, b) => a.isBefore(b) ? a : b);

  /// The text: a sent one whole, a received one as far as it has come.
  String get text => mine
      ? outs.map((o) => o.text).join()
      : [for (final p in pieces) p.text].join();

  /// A received text's pieces in order: what has come, and a line in
  /// place of each part that has not.
  List<({String text, bool missing})> get pieces {
    final out = <({String text, bool missing})>[];
    for (final (i, r) in ins.indexed) {
      if (r != null) {
        out.add((text: r.text, missing: false));
        continue;
      }
      final before = out.isEmpty || out.last.text.endsWith('\n') ? '' : '\n';
      final after = i == ins.length - 1 ? '' : '\n';
      out.add((
        text: '${before}part ${i + 1} of ${ins.length} not yet here$after',
        missing: true,
      ));
    }
    return out;
  }
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
  // A text's parts are one entry, where its first part to be queued or
  // to come stands. Ties keep sent before received and each list's own
  // order, which List.sort, not being stable, would not.
  final outs = <String, List<Outgoing>>{};
  final firstOut = <String, int>{};
  for (final (i, o) in outbox.indexed) {
    if (o.kind != OutKind.text || o.to == null) continue;
    final k = '${o.to}/${o.first}/${o.of}';
    (outs[k] ??= []).add(o);
    firstOut.putIfAbsent(k, () => i);
  }
  final ins = <String, List<Received?>>{};
  final firstIn = <String, int>{};
  for (final (i, r) in received.indexed) {
    if (r.from == null || r.body == null) continue;
    final k = '${r.from}/${r.first}/${r.of}';
    (ins[k] ??= List.filled(r.of, null))[r.part - 1] = r;
    firstIn.putIfAbsent(k, () => i);
  }
  final all = [
    for (final MapEntry(key: k, value: os) in outs.entries)
      (
        Entry.sent(os..sort((a, b) => a.part - b.part)),
        os.first.to!,
        0,
        firstOut[k]!,
      ),
    for (final MapEntry(key: k, value: rs) in ins.entries)
      (Entry.received(rs), rs.nonNulls.first.from!, 1, firstIn[k]!),
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
