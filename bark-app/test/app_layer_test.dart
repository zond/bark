// The app layer against bark-app-layer's vectors.json: the bytes the
// node and the Rust side make are the bytes libsodium here reads and,
// where nothing random enters, makes.

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/words.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

void main() {
  final v = jsonDecode(
    File('../bark-app-layer/vectors.json').readAsStringSync(),
  ) as Map<String, dynamic>;
  Map<String, dynamic> at(String k) => v[k] as Map<String, dynamic>;
  Uint8List h(Map<String, dynamic> m, String k) => parseHex(m[k] as String)!;
  late Sodium s;
  late Keys alice, bob;
  setUpAll(() async {
    s = await SodiumInit.init();
    alice = Keys.fromSeed(s, h(at('alice'), 'seed'));
    bob = Keys.fromSeed(s, h(at('bob'), 'seed'));
  });

  test('the word list is the Rust one', () {
    final rust = File('../bark-app-layer/src/words.txt').readAsLinesSync();
    expect(words, rust);
  });

  test('seed to phrase and keys', () {
    for (final (name, k) in [('alice', alice), ('bob', bob)]) {
      final m = at(name);
      expect(phraseOf(h(m, 'seed')).join(' '), m['phrase']);
      expect(seedOfPhrase((m['phrase'] as String).toUpperCase()), h(m, 'seed'));
      expect(hex(k.identity), m['identity']);
      expect(hex(k.pair.secretKey.extractBytes()), m['secret']);
      expect(hex(k.mailbox.extractBytes()), m['mailbox']);
    }
    final p = phraseOf(h(at('alice'), 'seed'));
    p[11] = p[11] == 'zoo' ? 'abandon' : 'zoo';
    expect(seedOfPhrase(p.join(' ')), isNull);
    expect(seedOfPhrase(p.sublist(1).join(' ')), isNull);
  });

  test('the card, its bytes and its text', () {
    final m = at('card');
    final c = PersonCard.parse(m['text'] as String);
    expect(hex(c.identity), m['identity']);
    expect(c.address.toString(), m['address']);
    expect(c.home, m['home']);
    expect(c.time, m['time']);
    expect(c.name, m['name']);
    expect(hex(c.encode()), m['bytes']);
    expect(c.toText(), m['text']);
    expect(PersonCard.parse((m['text'] as String).toLowerCase()).name, c.name);
  });

  test('boxed: made and opened as the Rust side does', () {
    final m = at('box_1251');
    final plain = writePlain(
      Header(
        m['greater'] as bool,
        m['sent'] as int,
        Address.parse(m['address'] as String),
      ),
      [TextRecord(m['text'] as String)],
    );
    expect(hex(plain), m['plaintext']);
    final ab = s.crypto.box.precalculate(
      publicKey: bob.identity,
      secretKey: alice.pair.secretKey,
    );
    expect(hex(sealBox(ab, h(m, 'nonce'), plain)), m['payload']);
    final ba = s.crypto.box.precalculate(
      publicKey: alice.identity,
      secretKey: bob.pair.secretKey,
    );
    final (hd, rs) = readPlain(openBox(ba, h(m, 'payload')));
    expect(hd.sent, m['sent']);
    expect(hd.greater, greater(alice.identity, bob.identity));
    expect((rs.single as TextRecord).text, m['text']);
  });

  test('boxed: an acknowledgement and a UTF-8 text', () {
    for (final name in ['box_utf8_ack', 'box_ack']) {
      final m = at(name);
      final ab = s.crypto.box.precalculate(
        publicKey: bob.identity,
        secretKey: alice.pair.secretKey,
      );
      final plain = openBox(ab, h(m, 'payload'));
      expect(hex(plain), m['plaintext']);
      final (hd, rs) = readPlain(plain);
      expect(hd.greater, greater(bob.identity, alice.identity));
      expect((rs.first as AckRecord).sent, m['acknowledges']);
      if (m['text'] != null) expect((rs.last as TextRecord).text, m['text']);
      final records = [
        AckRecord(m['acknowledges'] as int),
        if (m['text'] != null) TextRecord(m['text'] as String),
      ];
      expect(
        hex(writePlain(Header(hd.greater, hd.sent), records)),
        m['plaintext'],
      );
    }
  });

  test('another pair does not open a box', () {
    final carol = Keys.fromSeed(s, List.filled(16, 3));
    final cb = s.crypto.box.precalculate(
      publicKey: carol.identity,
      secretKey: bob.pair.secretKey,
    );
    expect(
      () => openBox(cb, h(at('box_1251'), 'payload')),
      throwsA(isA<BadPayload>()),
    );
  });

  test('an introduction opens, and one made here opens too', () {
    final m = at('intro');
    final plain = openIntro(s, bob, h(m, 'payload'));
    expect(hex(plain), m['plaintext']);
    final (_, rs) = readPlain(plain);
    final card = PersonCard.decode((rs.first as CardRecord).card);
    expect(card.identity, alice.identity);
    expect((rs.last as TextRecord).text, m['text']);
    final mine = sealIntro(s, bob.identity, plain);
    expect(openIntro(s, bob, mine), plain);
    expect(() => openIntro(s, alice, mine), throwsA(isA<BadPayload>()));
  });

  test('a fetch: made and opened', () {
    final m = at('fetch');
    final f = Fetch(
      m['sent'] as int,
      Address.parse(m['address'] as String)!,
      m['since'] as int,
    );
    expect(hex(f.seal(s, alice.mailbox, h(m, 'nonce'))), m['payload']);
    expect(h(m, 'payload').length, Fetch.len);
    final g = Fetch.open(s, alice.mailbox, h(m, 'payload'));
    expect(
      (g.sent, g.address.toString(), g.since),
      (f.sent, f.address.toString(), f.since),
    );
    expect(
      () => Fetch.open(s, bob.mailbox, h(m, 'payload')),
      throwsA(isA<BadPayload>()),
    );
  });

  test('a reply: made, opened in its round only, its number inside', () {
    final m = at('reply');
    final round = m['round'] as int;
    final place = m['place'] as int;
    final n = m['number'] as int;
    expect(hex(replyNonce(round, place)), m['nonce']);
    expect(
      hex(sealReply(s, alice.mailbox, round, place, n, h(m, 'item'))),
      m['payload'],
    );
    final o = openReply(s, alice.mailbox, round, h(m, 'payload'))!;
    expect((o.number, o.place, hex(o.item!)), (n, place, m['item']));
    expect(openReply(s, alice.mailbox, round + 1, h(m, 'payload')), isNull);
    expect(openReply(s, bob.mailbox, round, h(m, 'payload')), isNull);
  });

  test('nothing new: made, opened in its round only', () {
    final m = at('nothing_new');
    final round = m['round'] as int;
    expect(hex(replyNonce(round, 0)), m['nonce']);
    expect(hex(sealNothingNew(s, alice.mailbox, round)), m['payload']);
    expect(h(m, 'payload').length, nothingNewLen);
    expect(
      openReply(s, alice.mailbox, round, h(m, 'payload'))!.nothingNew,
      isTrue,
    );
    expect(openReply(s, alice.mailbox, round + 1, h(m, 'payload')), isNull);
  });

  test('windows-1251 when it holds the text', () {
    expect(encode1251('Їжак'), [0xaf, 0xe6, 0xe0, 0xea]);
    expect(encode1251('Їжак 🦔'), isNull);
    expect(decode1251([0x98]), isNull);
  });
}
