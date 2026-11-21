// The stored files across app versions: a file the app cannot read is
// set aside and the app starts, with the fact in the log.

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/identity_file.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/nodes.dart';
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
/// up, without a version.
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

  test(
    'an old me.json and contacts.json, of no version, are set aside',
    () async {
      final contacts = MemoryStore('contacts.json')..text = oldContacts;
      final c = Contacts(contacts);
      await c.load();
      expect(c.list, isEmpty);
      expect(contacts.aside, {'contacts.json.old-bad': oldContacts});
      final me = MemoryStore('me.json')..text = oldMe;
      final m = Me(me, sodium, () => at);
      await m.load();
      expect(hex(m.keys.seed), isNot('000102030405060708090a0b0c0d0e0f'));
      expect(me.aside, {'me.json.old-bad': oldMe});
    },
  );

  Outgoing text(int sent, String body) => Outgoing(
    Address.parse('47c0490ad9ff80fa4e00000001')!,
    Uint8List.fromList([...parseHex('4024e7d17894f3bfc974f62668')!, sent]),
    at: at,
    kind: OutKind.text,
    sent: sent,
    to: 'ab',
    body: body,
  );

  /// The lines of a log, its first naming the version.
  List<String> lines(MemoryStore s) => s.text!.split('\n');

  test('a current bark.json round-trips', () async {
    final store = MemoryStore('bark.json');
    final s = Session(store: store, now: () => at);
    await s.queue(text(7, 'hello'));
    await s.addReceived(Received(1, [1, 2, 3], at, from: 'cd', sent: 5));
    await s.setStopped(true);
    final saved = store.text!;
    expect(lines(store).first, '{"version":${Session.version}}');
    final again = Session(store: store, now: () => at);
    await again.load();
    expect(store.fault, isNull);
    expect(store.text, saved);
    expect(again.outbox.single.body, 'hello');
    expect(again.received.single.from, 'cd');
    expect(again.stopped, isTrue);
  });

  test('a change adds a line, and the file is written whole once the '
      'changes in it outnumber twice what it holds', () async {
    final store = MemoryStore('bark.json');
    final s = Session(store: store, now: () => at);
    final o = text(7, 'hello');
    await s.queue(o);
    // The header, the message, the state, and the end of the last line.
    expect(lines(store), hasLength(4));
    final before = store.text!;
    o.state = OutState.taken;
    await s.save(out: [o]);
    expect(store.text, startsWith(before));
    expect(lines(store), hasLength(5));
    expect(lines(store)[3], contains('"state":"taken"'));
    for (var i = 0; i < 20; i++) {
      await s.save(out: [o]);
      // Two held, the message and the state: never more than four changes.
      expect(lines(store).length, lessThanOrEqualTo(1 + 4 + 1));
    }
    final again = Session(store: store, now: () => at);
    await again.load();
    expect(again.outbox.single.state, OutState.taken);
  });

  test('a message dropped stays dropped', () async {
    final store = MemoryStore('bark.json');
    final s = Session(store: store, now: () => at);
    final a = text(7, 'a'), b = text(8, 'b');
    await s.queue(a);
    await s.queue(b);
    await s.drop(a);
    final again = Session(store: store, now: () => at);
    await again.load();
    expect(again.outbox.map((o) => o.body), ['b']);
    await again.queue(text(9, 'c'));
    final third = Session(store: store, now: () => at);
    await third.load();
    expect(third.outbox.map((o) => o.body), ['b', 'c']);
  });

  test('a write cut off loses that change alone, and the file is written '
      'whole again', () async {
    final store = MemoryStore('bark.json');
    final s = Session(store: store, now: () => at);
    await s.queue(text(7, 'kept'));
    await s.queue(text(8, 'cut'));
    final t = store.text!;
    // A kill halfway through the last line.
    store.text = t.substring(0, t.length - 20);
    final again = Session(store: store, now: () => at);
    await again.load();
    expect(store.fault, isNull);
    expect(again.outbox.map((o) => o.body), ['kept']);
    expect(store.text, endsWith('\n'));
    expect(store.text, isNot(contains('cut')));
    // What follows lands after whole lines.
    await again.queue(text(9, 'after'));
    final third = Session(store: store, now: () => at);
    await third.load();
    expect(third.outbox.map((o) => o.body), ['kept', 'after']);
  });

  test('a bark.json of version 3, the whole state as one object, reads '
      'and is written afresh as a log', () async {
    final old = jsonEncode({
      'version': 3,
      'outbox': [text(7, 'hello').toJson()],
      'received': [
        Received(
          1,
          [1, 2, 3],
          at,
          from: 'cd',
          sent: 5,
          reaction: '👍',
        ).toJson(),
      ],
      'nodes': [(NodeRecord('bark-fa4e00000001')..paired = true).toJson()],
      'stopped': true,
      'reactionsUsed': {'👍': 2},
    });
    final store = MemoryStore('bark.json')..text = old;
    final s = Session(store: store, now: () => at);
    await s.load();
    expect(store.fault, isNull);
    expect(store.aside, isEmpty);
    expect(s.outbox.single.body, 'hello');
    expect(s.received.single.reaction, '👍');
    expect(s.nodes.paired, {'bark-fa4e00000001'});
    expect(s.stopped, isTrue);
    expect(s.reactionsUsed, {'👍': 2});
    expect(lines(store).first, '{"version":${Session.version}}');
    final again = Session(store: store, now: () => at);
    await again.load();
    expect(again.outbox.single.body, 'hello');
    expect(again.nodes.paired, {'bark-fa4e00000001'});
    expect(again.reactionsUsed, {'👍': 2});
  });

  test('a file of a future version is set aside', () async {
    final future = '{"version":${Session.version + 1}}\n';
    final store = MemoryStore('bark.json')..text = future;
    final s = Session(store: store, now: () => at);
    await s.load();
    expect(store.aside, {'bark.json.old-${Session.version + 1}': future});
    expect(store.fault, contains('version ${Session.version + 1}'));
    // The next change writes the current version afresh.
    await s.setStopped(true);
    expect(lines(store).first, '{"version":${Session.version}}');
  });

  test('a line that does not read in the middle sets the file aside', () async {
    final store = MemoryStore('bark.json');
    final s = Session(store: store, now: () => at);
    await s.queue(text(7, 'a'));
    final bad = store.text!.replaceFirst('\n', '\n[{"out":"x"}]\n');
    store.text = bad;
    await Session(store: store, now: () => at).load();
    expect(store.aside, {'bark.json.old-bad': bad});
  });

  test('a current me.json round-trips, every field', () async {
    final store = MemoryStore('me.json');
    final m = Me(store, sodium, () => at);
    await m.load();
    await m.setName('Ann');
    final home = Address.parse('47c0490ad9ff80fa4e00000001')!;
    await m.setHome(home, key: Uint8List(32));
    await m.setNodeAddress(home);
    await m.nextSents(3);
    await m.setCardSent(m.cardTime);
    await m.setBackedUp();
    await m.setAskedToNotify();
    await m.setAskedBluetooth();
    await m.setFetchQueued(true);
    await m.fetched(9);
    await m.setOwner(true);
    await m.setLeaving(Leaving(home, Uint8List(32))..round = 9);
    final saved = store.text!;
    final again = Me(store, sodium, () => at);
    await again.load();
    expect(store.fault, isNull);
    expect(again.key, m.key);
    expect(again.name, 'Ann');
    expect(again.home.toString(), home.toString());
    expect(again.givenKey, Uint8List(32));
    expect(again.rounds, [9]);
    expect(again.leaving!.round, 9);
    await again.setName('Ann');
    expect(store.text, saved);
  });

  test(
    'a restore and a move-in free the keys of the identity replaced',
    () async {
      final m = Me(MemoryStore('me.json'), sodium, () => at);
      await m.load();
      final first = m.keys;
      await m.restore(Uint8List(16));
      expect(first.disposed, isTrue);
      final second = m.keys;
      expect(second.disposed, isFalse);
      expect(second.mailbox.extractBytes(), hasLength(32));
      await m.moveIn(
        IdentityFile(
          seed: Uint8List.fromList(List.filled(16, 1)),
          name: 'Ann',
          home: null,
          nodeAddress: null,
          nodeAt: 0,
          cardTime: 0,
          cardSentTime: 0,
          lastSent: 0,
          backedUp: true,
          remaining: 0,
          rounds: const [],
          contacts: const [],
          archived: const {},
          nodes: const [],
          messages: null,
        ),
      );
      expect(second.disposed, isTrue);
      expect(m.keys.mailbox.extractBytes(), hasLength(32));
    },
  );

  test('a me.json set aside gives a new identity and keeps the old', () async {
    final bad = oldMe.replaceFirst('{', '{"version":${Me.version},');
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
      startsWith('Stored me.json did not read'),
      startsWith('Stored bark.json did not read'),
      startsWith('Stored contacts.json did not read'),
    ]);
    app.dispose();
  });
  test(
    'two writes of one file at once: both end, the later one stays',
    () async {
      final dir = await Directory.systemTemp.createTemp('bark-store');
      addTearDown(() => dir.delete(recursive: true));
      final store = FileStore('x.json', dir: () async => dir);
      await Future.wait([for (var i = 0; i < 20; i++) store.write('{"n":$i}')]);
      expect(await store.read(), '{"n":19}');
      expect(dir.listSync().map((f) => f.uri.pathSegments.last), ['x.json']);
    },
  );

  test(
    'an append waits for the write before it and lands in its file',
    () async {
      final dir = await Directory.systemTemp.createTemp('bark-store');
      addTearDown(() => dir.delete(recursive: true));
      final store = FileStore('x.log', dir: () async => dir);
      await Future.wait([
        store.write('a\n'),
        store.append('b\n'),
        store.write('c\n'),
        store.append('d\n'),
        store.append('e\n'),
      ]);
      expect(await store.read(), 'c\nd\ne\n');
      expect(dir.listSync().map((f) => f.uri.pathSegments.last), ['x.log']);
    },
  );
}
