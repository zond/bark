// The identity file: sealed under a password by Argon2id and opened again,
// refused for a wrong password, a changed byte, another file and another
// layout; and a person moved from one phone to another, with their
// messages and without, their home's key and the nodes to pair again.

import 'dart:convert';
import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/app_layer.dart' show Keys, seedLen;
import 'package:bark_app/contacts.dart';
import 'package:bark_app/identity_file.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'fake_ble.dart';
import 'fake_node.dart';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 21);

  /// A key of the password and salt made at once, for the tests that run
  /// in fake time, where another isolate's answer never comes; Argon2id
  /// itself is tried apart.
  Future<Uint8List> quickKey(String password, Uint8List salt) async =>
      sodium.crypto.genericHash(
        message: Uint8List.fromList([...utf8.encode(password), ...salt]),
        outLen: 32,
      );

  IdentityFile small({Uint8List? key}) {
    final seed = Uint8List.fromList(List.generate(seedLen, (i) => i));
    final home = Address.parse('01020304050607fa4e00000001')!;
    return IdentityFile(
      seed: seed,
      name: 'Ann',
      home: (
        address: home,
        key: key ?? Keys.fromSeed(sodium, seed).mailbox.extractBytes(),
      ),
      nodeAddress: home,
      nodeAt: 100,
      cardTime: 5,
      cardSentTime: 4,
      lastSent: 1234,
      backedUp: true,
      remaining: 2,
      rounds: [1200, 1100],
      contacts: const [],
      archived: const {},
      nodes: const [
        (name: 'bark-fa4e00000001', paired: true, lost: false, ringAfter: 7),
      ],
      messages: null,
    );
  }

  group('the file', () {
    test('its key is Argon2id of 3 passes over 64 MiB', () async {
      // Checked against another implementation of Argon2id, 2026-11-21.
      final k = await argon2id(
        'pw',
        Uint8List.fromList(List.generate(saltLen, (i) => i)),
      );
      expect(
        hex(k),
        '62a65cfabb64dad458d4ae255b15e2a90e938a1f5e81b171ad928a6848af870c',
      );
    });

    test('opens under its password by Argon2id', () async {
      final f = await small().seal(sodium, argon2id, 'correct horse');
      expect(f.sublist(0, magic.length), magic);
      expect(f[magic.length], identityFileVersion);
      final o = await IdentityFile.open(sodium, argon2id, 'correct horse', f);
      expect(jsonEncode(o.toJson()), jsonEncode(small().toJson()));
      await expectLater(
        IdentityFile.open(sodium, argon2id, 'wrong horse', f),
        throwsA(isA<WrongPassword>()),
      );
    });

    test('two saves of one person differ, by their salt and nonce', () async {
      final a = await small().seal(sodium, quickKey, 'pw');
      final b = await small().seal(sodium, quickKey, 'pw');
      expect(a, isNot(b));
    });

    test('a changed byte anywhere, header or contents, fails', () async {
      final f = await small().seal(sodium, quickKey, 'pw');
      for (final i in [
        magic.length + 1,
        headerLen - 1,
        headerLen,
        f.length - 1,
      ]) {
        final d = Uint8List.fromList(f)..[i] ^= 1;
        await expectLater(
          IdentityFile.open(sodium, quickKey, 'pw', d),
          throwsA(isA<WrongPassword>()),
          reason: 'byte $i',
        );
      }
      await expectLater(
        IdentityFile.open(sodium, quickKey, 'pw', f.sublist(0, headerLen + 3)),
        throwsA(isA<WrongPassword>()),
      );
    });

    test('another file is not one', () async {
      for (final other in [
        Uint8List(0),
        Uint8List.fromList(magic),
        Uint8List.fromList(utf8.encode('{"version":1}')),
      ]) {
        await expectLater(
          IdentityFile.open(sodium, quickKey, 'pw', other),
          throwsA(isA<NotAnIdentityFile>()),
        );
      }
    });

    test('a file of another layout version is refused as such', () async {
      final f = await small().seal(sodium, quickKey, 'pw');
      final later = Uint8List.fromList(f)..[magic.length] = 2;
      await expectLater(
        IdentityFile.open(sodium, quickKey, 'pw', later),
        throwsA(isA<OtherLayout>()),
      );
      expect('${const OtherLayout()}', contains('another version'));
    });

    test('a home with a key the identity does not make is refused', () async {
      final f = await small(key: Uint8List(32)).seal(sodium, quickKey, 'pw');
      await expectLater(
        IdentityFile.open(sodium, quickKey, 'pw', f),
        throwsA(isA<OtherLayout>()),
      );
    });
  });

  group('moving to a new phone', () {
    AppModel phone(DateTime Function() now) => AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, now),
      session: Session(store: MemoryStore(), now: now),
      contacts: Contacts(MemoryStore()),
      now: now,
      passwordKey: quickKey,
    );

    /// The old phone, with a name, its home at the fake node and the fake
    /// friend's two first messages; and a new phone, of another identity.
    void run(void Function(FakeAsync, AppModel old, AppModel fresh) body) {
      fakeAsync((async) {
        DateTime now() => start.add(async.elapsed);
        final old = phone(now);
        final fresh = phone(now);
        old.load();
        fresh.load();
        async.flushMicrotasks();
        old.me.setName('Ann');
        old.connectFake();
        async.flushMicrotasks();
        old.makeHome(old.session.address!);
        async.elapse(const Duration(seconds: 2));
        expect(old.contacts.list, hasLength(1));
        expect(old.session.received, isNotEmpty);
        body(async, old, fresh);
        old.disposeWithNode();
        fresh.dispose();
      });
    }

    T done<T>(FakeAsync async, Future<T> f) {
      T? r;
      Object? e;
      f.then(
        (v) {
          r = v;
        },
        onError: (Object x) {
          e = x;
        },
      );
      async.flushMicrotasks();
      if (e != null) throw e!;
      return r as T;
    }

    test('with the messages: the same person, contacts and texts', () {
      run((async, old, fresh) {
        final file = done(async, old.identityFile('pw', messages: true));
        done(async, fresh.restoreFile(file, 'pw'));
        expect(fresh.me.key, old.me.key);
        expect(fresh.me.name, 'Ann');
        expect(fresh.me.lastSent, old.me.lastSent);
        expect(fresh.me.card.toText(), old.me.card.toText());
        expect(
          jsonEncode([for (final c in fresh.contacts.list) c.toJson()]),
          jsonEncode([for (final c in old.contacts.list) c.toJson()]),
        );
        expect(
          jsonEncode([for (final r in fresh.session.received) r.toJson()]),
          jsonEncode([for (final r in old.session.received) r.toJson()]),
        );
        expect(fresh.session.outbox, hasLength(old.session.outbox.length));
        // Kept in the new phone's own files.
        expect((fresh.me.store as MemoryStore).state!['name'], 'Ann');
      });
    });

    test('without the messages: none, the contacts still there', () {
      run((async, old, fresh) {
        final file = done(async, old.identityFile('pw', messages: false));
        done(async, fresh.restoreFile(file, 'pw'));
        expect(fresh.me.key, old.me.key);
        expect(fresh.contacts.list, hasLength(1));
        expect(fresh.session.received, isEmpty);
        expect(fresh.session.outbox, isEmpty);
      });
    });

    test('the home and its key survive, its node to pair again', () {
      run((async, old, fresh) {
        final file = done(async, old.identityFile('pw', messages: false));
        done(async, fresh.restoreFile(file, 'pw'));
        final home = nodeName(FakeNode.id);
        expect(fresh.me.home.toString(), old.me.home.toString());
        expect(fresh.homeName, home);
        expect(fresh.me.keys.mailbox.extractBytes(), old.fakeNode!.homeKey);
        final r = fresh.nodes[home]!;
        expect((r.paired, r.lost, r.moved), (true, true, true));
        expect(fresh.nodes.own, isEmpty);
        expect(fresh.nodes.moved, {home});
        // Kept as such across a restart.
        final again = Session(store: fresh.session.store, now: fresh.now);
        again.load();
        async.flushMicrotasks();
        expect(again.nodes[home]!.moved, isTrue);
        again.dispose();
        // Pairing anew makes it the phone's own again.
        done(async, fresh.session.setPaired(home));
        expect(fresh.nodes[home]!.moved, isFalse);
        expect(fresh.nodes.own, {home});
      });
    });

    test('a wrong password changes nothing', () {
      run((async, old, fresh) {
        final file = done(async, old.identityFile('pw', messages: true));
        final before = fresh.me.key;
        expect(
          () => done(async, fresh.restoreFile(file, 'not it')),
          throwsA(isA<WrongPassword>()),
        );
        expect(fresh.me.key, before);
        expect(fresh.contacts.list, isEmpty);
        expect(fresh.nodes.paired, isEmpty);
      });
    });

    test('a damaged file changes nothing', () {
      run((async, old, fresh) {
        final file = done(async, old.identityFile('pw', messages: true));
        file[file.length ~/ 2] ^= 0x80;
        final before = fresh.me.key;
        expect(
          () => done(async, fresh.restoreFile(file, 'pw')),
          throwsA(isA<WrongPassword>()),
        );
        expect(fresh.me.key, before);
        expect(fresh.contacts.list, isEmpty);
      });
    });

    test(
      'restoring ends the link, which belonged to the identity replaced',
      () {
        run((async, old, fresh) {
          final file = done(async, fresh.identityFile('pw', messages: false));
          expect(old.session.connected, isTrue);
          done(async, old.restoreFile(file, 'pw'));
          expect(old.session.connected, isFalse);
          expect(old.me.key, fresh.me.key);
          expect(old.contacts.list, isEmpty);
          expect(old.session.received, isEmpty);
          // This phone's own pairing stays: Android holds its bond.
          expect(old.nodes.own, {nodeName(FakeNode.id)});
        });
      },
    );
  });
}
