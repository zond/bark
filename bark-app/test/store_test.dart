// The stored files across app versions: a file the app cannot read is
// set aside and the app starts, with the fact in the log.

import 'dart:convert';

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// `bark.json` from a phone running a build before the outbox kept a
/// message's time, kind and text, and before files had a version.
const oldBark =
    '{"outbox":[{"dst":"47c0490ad9ff80fa4e00000001",'
    '"payload":"4024e7d17894f3bfc974f6266865687521","state":"taken",'
    '"retryAt":null,"error":null}],"received":[{"id":2499783849,'
    '"payload":"489d406545d7312dbde12d5848656c6c6f2066726f6d207468652066616b'
    '65206e6f64652e","at":"2026-11-05T16:38:21.640390"},{"id":4195522374,'
    '"payload":"32d4d5dbfcb6e7dc115e125f41207365636f6e64206d6573736167652c20'
    '77616974696e6720696e2074686520696e626f782e",'
    '"at":"2026-11-05T16:38:21.855790"},{"id":2126049146,'
    '"payload":"7b59e1c04a442a3057aaf14b4563686f3a206865687521",'
    '"at":"2026-11-05T16:39:51.106508"}]}';

/// `contacts.json` and `me.json` as that build wrote them, the seed made
/// up: without a version, and read as version 1. Contacts are at version
/// 2 since their cards are of card version 2.
const oldContacts =
    '{"contacts":[],"myName":"Martin","myAddress":"47c0520ada4ec0bae6e6cbf4f5"}';
const oldMe =
    '{"seed":"000102030405060708090a0b0c0d0e0f","name":"","home":null,'
    '"nodeAddress":null,"cardTime":0,"lastSent":0,"backedUp":false,'
    '"since":0,"above":[],"rounds":[]}';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final at = DateTime(2026, 11, 6, 8);

  test('an old bark.json is set aside and the session starts empty', () async {
    final store = MemoryStore('bark.json')..text = oldBark;
    final s = Session(store: store, now: () => at);
    await s.load();
    expect(s.outbox, isEmpty);
    expect(s.received, isEmpty);
    expect(store.text, isNull);
    expect(store.aside, {'bark.json.old-bad': oldBark});
    expect(store.fault, startsWith('Stored bark.json did not read'));
  });

  test('the old me.json is read, the old contacts.json set aside', () async {
    final contacts = MemoryStore('contacts.json')..text = oldContacts;
    final c = Contacts(contacts);
    await c.load();
    expect(c.list, isEmpty);
    expect(contacts.aside, {'contacts.json.old-1': oldContacts});
    final me = MemoryStore('me.json')..text = oldMe;
    final m = Me(me, sodium, () => at);
    await m.load();
    expect(hex(m.keys.seed), '000102030405060708090a0b0c0d0e0f');
    expect(me.fault, isNull);
  });

  test('a current bark.json round-trips', () async {
    final store = MemoryStore('bark.json');
    final s = Session(store: store, now: () => at);
    final dst = Address.parse('47c0490ad9ff80fa4e00000001')!;
    await s.queue(
      Outgoing(
        dst,
        parseHex('4024e7d17894f3bfc974f6266865687521')!,
        at: at,
        kind: OutKind.text,
        sent: 7,
        to: 'ab',
        body: 'hello',
      ),
    );
    await s.addReceived(Received(1, [1, 2, 3], at, from: 'cd', sent: 5));
    final saved = store.text!;
    expect(store.state!['version'], Session.version);
    final again = Session(store: store, now: () => at);
    await again.load();
    expect(store.fault, isNull);
    await again.save();
    expect(store.text, saved);
    expect(again.outbox.single.body, 'hello');
    expect(again.received.single.from, 'cd');
  });

  test('a file of a future version is set aside', () async {
    final future = jsonEncode({
      'version': Session.version + 1,
      'outbox': [],
      'received': [],
    });
    final store = MemoryStore('bark.json')..text = future;
    final s = Session(store: store, now: () => at);
    await s.load();
    expect(store.aside, {'bark.json.old-${Session.version + 1}': future});
    expect(store.fault, contains('version ${Session.version + 1}'));
    // The next save writes the current version afresh.
    await s.save();
    expect(store.state!['version'], Session.version);
  });

  test('a me.json set aside gives a new identity and keeps the old', () async {
    final bad = oldMe.replaceFirst('"lastSent":0', '"lastSent":"0"');
    final store = MemoryStore('me.json')..text = bad;
    final m = Me(store, sodium, () => at);
    await m.load();
    expect(hex(m.keys.seed), isNot('000102030405060708090a0b0c0d0e0f'));
    expect(store.aside['me.json.old-bad'], bad);
    expect(store.state!['seed'], hex(m.keys.seed));
  });

  test('a second fault keeps the first file set aside', () async {
    final store = MemoryStore('me.json')..text = '{';
    await Me(store, sodium, () => at).load();
    store.text = '[';
    await Me(store, sodium, () => at).load();
    expect(store.aside, {'me.json.old-bad': '{', 'me.json.old-bad-2': '['});
    expect(store.fault, contains('kept as me.json.old-bad-2'));
  });

  test('the app starts on old files and logs each once', () async {
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore('me.json')..text = oldMe, sodium, () => at),
      session: Session(
        store: MemoryStore('bark.json')..text = oldBark,
        now: () => at,
      ),
      contacts: Contacts(MemoryStore('contacts.json')..text = oldContacts),
      now: () => at,
    );
    await app.load();
    final lines = app.session.log.map((l) => l.text).toList();
    expect(lines, [
      startsWith('Stored bark.json did not read'),
      startsWith('Stored contacts.json is of version 1'),
    ]);
    app.dispose();
  });
}
