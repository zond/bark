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
  late Address home, away;
  setUpAll(() async {
    s = await SodiumInit.init();
    alice = Keys.fromSeed(s, h(at('alice'), 'seed'));
    bob = Keys.fromSeed(s, h(at('bob'), 'seed'));
    home = PersonCard.parse(at('card')['text'] as String).home!;
    away = Address.parse(at('fetch')['address'] as String)!;
  });
  int tag(Keys k, Address at) => recipientTag(s, k.identity, at.id);

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

  test('the recipient tag is the Rust one, keyed and per node', () {
    for (final (name, k) in [('alice', alice), ('bob', bob)]) {
      final m = at(name);
      expect(tag(k, home), m['recipient_home']);
      expect(tag(k, away), m['recipient_away']);
    }
    // libsodium's generichash keyed with the identity, its first byte.
    final key = SecureKey.fromList(s, alice.identity);
    expect(
      tag(alice, away),
      s.crypto.genericHash(message: away.id, outLen: 16, key: key)[0],
    );
    key.dispose();
  });

  test('the card, its bytes and its text, with a home and without', () {
    for (final name in ['card', 'card_no_home']) {
      final m = at(name);
      final c = PersonCard.parse(m['text'] as String);
      expect(hex(c.identity), m['identity']);
      expect(c.home?.toString(), m['home']);
      expect(c.now?.toString(), m['now']);
      expect(c.time, m['time']);
      expect(c.name, m['name']);
      expect(hex(c.encode()), m['bytes']);
      expect(c.toText(), m['text']);
      expect(
        PersonCard.parse((m['text'] as String).toLowerCase()).name,
        c.name,
      );
    }
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
    final ab = SivKeys.pair(s, bob.identity, alice.pair.secretKey);
    final payload = sealBox(ab, tag(bob, home), plain);
    expect(hex(payload), m['payload']);
    expect(payload[0], 0x20);
    expect(payload[1], at('bob')['recipient_home']);
    final ba = SivKeys.pair(s, alice.identity, bob.pair.secretKey);
    final (hd, rs) = readPlain(openBox(ba, h(m, 'payload')));
    expect(hd.sent, m['sent']);
    expect(hd.greater, greater(alice.identity, bob.identity));
    expect((rs.single as TextRecord).text, m['text']);
  });

  test('boxed: a UTF-8 text, and reactions up to the longest emoji', () {
    for (final name in ['box_utf8', 'box_reaction', 'box_heart', 'box_kiss']) {
      final m = at(name);
      final ab = SivKeys.pair(s, bob.identity, alice.pair.secretKey);
      final plain = openBox(ab, h(m, 'payload'));
      expect(hex(plain), m['plaintext']);
      expect(h(m, 'payload')[1], at('alice')['recipient_away']);
      final (hd, rs) = readPlain(plain);
      expect(hd.greater, greater(bob.identity, alice.identity));
      final Record record;
      if (m['text'] != null) {
        expect((rs.single as TextRecord).text, m['text']);
        record = TextRecord(m['text'] as String);
      } else {
        final r = rs.single as ReactionRecord;
        expect(r.sent, m['reacts_to']);
        expect(r.emoji, m['reaction']);
        record = ReactionRecord(r.sent, r.emoji);
      }
      expect(
        hex(writePlain(Header(hd.greater, hd.sent), [record])),
        m['plaintext'],
      );
    }
    expect(utf8.encode(at('box_kiss')['reaction'] as String).length, 35);
  });

  test('a reaction is one character, and last', () {
    const h = Header(false, 7);
    for (final e in ['🇺🇦', '#️⃣', '👨‍👩‍👧', 'é']) {
      final (_, rs) = readPlain(writePlain(h, [ReactionRecord(5, e)]));
      expect((rs.single as ReactionRecord).emoji, e);
    }
    // None, two, or a joined one past the bound.
    for (final e in [
      '',
      '👍👍',
      'ok',
      '${at('box_kiss')['reaction']}\u{fe0f}',
    ]) {
      expect(
        () => writePlain(h, [ReactionRecord(5, e)]),
        throwsA(isA<BadPayload>()),
      );
      expect(
        () => readPlain([0, 0, 0, 0, 7, 3, 0, 0, 0, 5, ...utf8.encode(e)]),
        throwsA(isA<BadPayload>()),
      );
    }
    expect(
      () =>
          writePlain(h, [const ReactionRecord(5, '👍'), const TextRecord('x')]),
      throwsA(isA<BadPayload>()),
    );
    // A type no longer used.
    expect(
      () => readPlain([0, 0, 0, 0, 7, 7, 0, 0, 0, 5]),
      throwsA(isA<BadPayload>()),
    );
  });

  test('another pair does not open a box', () {
    final carol = Keys.fromSeed(s, List.filled(16, 3));
    final cb = SivKeys.pair(s, carol.identity, bob.pair.secretKey);
    expect(
      () => openBox(cb, h(at('box_1251'), 'payload')),
      throwsA(isA<BadPayload>()),
    );
  });

  test(
    'the pair keys are the Rust ones from either side, and the mailbox\'s',
    () {
      final m = at('keys');
      for (final k in [
        SivKeys.pair(s, bob.identity, alice.pair.secretKey),
        SivKeys.pair(s, alice.identity, bob.pair.secretKey),
      ]) {
        expect(hex(k.mac.extractBytes()), m['pair_mac']);
        expect(hex(k.enc.extractBytes()), m['pair_enc']);
      }
      final mbx = SivKeys.of(s, alice.mailbox);
      expect(hex(mbx.mac.extractBytes()), m['mailbox_mac']);
      expect(hex(mbx.enc.extractBytes()), m['mailbox_enc']);
      expect(
        hex(replyKey(s, alice.mailbox).extractBytes()),
        m['mailbox_reply'],
      );
      // A public key of small order has no shared key.
      expect(
        () => SivKeys.pair(s, List.filled(keyLen, 0), alice.pair.secretKey),
        throwsA(isA<BadPayload>()),
      );
    },
  );

  test('a box: one plaintext makes one payload, the same both ways', () {
    final m = at('box_1251');
    final ab = SivKeys.pair(s, bob.identity, alice.pair.secretKey);
    final ba = SivKeys.pair(s, alice.identity, bob.pair.secretKey);
    final plain = h(m, 'plaintext');
    expect(
      hex(sealBox(ba, tag(bob, home), plain)),
      hex(sealBox(ab, tag(bob, home), plain)),
    );
    // Bob to Alice opens under Alice's pair with Bob, the same keys.
    expect(
      hex(openBox(ab, h(at('box_utf8'), 'payload'))),
      at('box_utf8')['plaintext'],
    );
  });

  test('a forged siv, a flipped bit and another pair are refused', () {
    final m = at('box_refused');
    final ba = SivKeys.pair(s, alice.identity, bob.pair.secretKey);
    for (final k in ['forged_siv', 'flipped', 'other_pair']) {
      expect(() => openBox(ba, h(m, k)), throwsA(isA<BadPayload>()), reason: k);
    }
    final carol = Keys.fromSeed(s, h(m, 'carol_seed'));
    final bc = SivKeys.pair(s, carol.identity, bob.pair.secretKey);
    expect(hex(openBox(bc, h(m, 'other_pair'))), at('box_1251')['plaintext']);
    final f = at('fetch_refused');
    for (final k in ['forged_siv', 'flipped', 'other_key', 'other_node']) {
      expect(
        () => Fetch.open(s, alice.mailbox, home.id, h(f, k)),
        throwsA(isA<BadPayload>()),
        reason: k,
      );
    }
    expect(
      Fetch.open(s, bob.mailbox, home.id, h(f, 'other_key')).sent,
      at('fetch')['sent'],
    );
    expect(
      Fetch.open(s, alice.mailbox, away.id, h(f, 'other_node')).sent,
      at('fetch')['sent'],
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
    expect(h(m, 'payload')[1], at('bob')['recipient_home']);
    final mine = sealIntro(s, bob.identity, tag(bob, home), plain);
    expect(mine[1], tag(bob, home));
    expect(openIntro(s, bob, mine), plain);
    expect(() => openIntro(s, alice, mine), throwsA(isA<BadPayload>()));
  });

  test('a fetch: made and opened', () {
    final m = at('fetch');
    final f = Fetch(m['sent'] as int, away, tag(alice, away));
    expect(f.recipient, m['recipient']);
    final homeTag = tag(alice, home);
    expect(homeTag, m['home']);
    expect(hex(f.seal(s, alice.mailbox, home.id, homeTag)), m['payload']);
    expect(h(m, 'payload').length, Fetch.len);
    expect(Fetch.len, boxOverhead + Fetch.plainLen);
    final g = Fetch.open(s, alice.mailbox, home.id, h(m, 'payload'));
    expect(
      (g.sent, g.address.toString(), g.recipient),
      (f.sent, f.address.toString(), f.recipient),
    );
    expect(
      () => Fetch.open(s, bob.mailbox, home.id, h(m, 'payload')),
      throwsA(isA<BadPayload>()),
    );
  });

  test('a summary: made, opened in its round only, nothing new its zero', () {
    final to = at('fetch')['recipient'] as int;
    for (final (name, items) in [('summary', 2), ('nothing_new', 0)]) {
      final m = at(name);
      final round = m['round'] as int;
      expect(hex(replyNonce(round, 0)), m['nonce']);
      final want = RoundSummary(items, m['remaining'] as int? ?? 0);
      expect(hex(sealSummary(s, alice.mailbox, to, round, want)), m['payload']);
      expect(h(m, 'payload').length, summaryLen);
      final o = openSummary(s, alice.mailbox, round, h(m, 'payload'))!;
      expect((o.items, o.remaining, o.nothingNew), (items, 0, items == 0));
      expect(openSummary(s, alice.mailbox, round + 1, h(m, 'payload')), isNull);
      expect(openSummary(s, bob.mailbox, round, h(m, 'payload')), isNull);
      final flipped = h(m, 'payload')..[summaryLen - 1] ^= 1;
      expect(openSummary(s, alice.mailbox, round, flipped), isNull);
    }
  });

  test('an item reply: made and opened as the Rust side does, and it opens '
      'only in its round', () {
    final to = at('fetch')['recipient'] as int;
    final ab = SivKeys.pair(s, bob.identity, alice.pair.secretKey);
    for (final name in ['reply_box', 'reply_intro']) {
      final m = at(name);
      final round = m['round'] as int;
      final place = m['place'] as int;
      expect(hex(replyNonce(round, place)), m['nonce']);
      expect(
        hex(sealItem(s, alice.mailbox, to, round, place, h(m, 'item'))),
        m['payload'],
      );
      expect(h(m, 'payload').length, h(m, 'item').length + replyOverhead);
      final item = openItem(s, alice.mailbox, round, h(m, 'payload'))!;
      expect(hex(item), m['opened']);
      expect(item[1], to);
      expect(item[0], h(m, 'item')[0]);
      Uint8List open(List<int> p) =>
          name == 'reply_box' ? openBox(ab, p) : openIntro(s, alice, p);
      final (_, rs) = readPlain(open(item));
      expect(rs.last, isA<TextRecord>());
      // Another round, or another key, gives what does not open.
      for (final (key, r) in [
        (alice.mailbox, round + 1),
        (bob.mailbox, round),
      ]) {
        expect(
          () => open(openItem(s, key, r, h(m, 'payload'))!),
          throwsA(isA<BadPayload>()),
        );
      }
    }
    // No place 0 nor past the batch, and no other form, goes as an item.
    final box = h(at('reply_box'), 'item');
    for (final place in [0, maxReplies + 1]) {
      expect(
        () => sealItem(s, alice.mailbox, to, 1, place, box),
        throwsA(isA<BadPayload>()),
      );
    }
    expect(
      () => sealItem(s, alice.mailbox, to, 1, 1, h(at('fetch'), 'payload')),
      throwsA(isA<BadPayload>()),
    );
    expect(openItem(s, alice.mailbox, 1, h(at('summary'), 'payload')), isNull);
  });

  test('windows-1251 when it holds the text', () {
    expect(encode1251('Їжак'), [0xaf, 0xe6, 0xe0, 0xea]);
    expect(encode1251('Їжак 🦔'), isNull);
    expect(decode1251([0x98]), isNull);
  });

  test('a split text: the parts and their bytes are the Rust ones', () {
    final m = at('box_parts');
    final room = m['room'] as int;
    final parts = (m['parts'] as List).cast<Map<String, dynamic>>();
    // The room of a whole text in a box without an address, as the Rust
    // side has it: one byte more than a part's.
    expect(room + 1, maxAppPayload - boxOverhead - 5 - 1);
    expect((maxAppPayload, room + 1), (223, 199));
    expect(splitText(m['text'] as String, room + 1), [
      for (final p in parts) p['text'],
    ]);
    final ab = SivKeys.pair(s, bob.identity, alice.pair.secretKey);
    final ba = SivKeys.pair(s, alice.identity, bob.pair.secretKey);
    for (final (i, p) in parts.indexed) {
      final hd = Header(
        greater(alice.identity, bob.identity),
        p['sent'] as int,
      );
      final plain = writePlain(hd, [
        TextRecord(p['text'] as String, i + 1, parts.length),
      ]);
      expect(hex(plain), p['plaintext']);
      expect(hex(sealBox(ab, tag(bob, home), plain)), p['payload']);
      final (got, rs) = readPlain(openBox(ba, h(p, 'payload')));
      final t = rs.single as TextRecord;
      expect(
        (got.sent, t.text, t.part, t.of),
        (p['sent'], p['text'], i + 1, parts.length),
      );
    }
    // A part of no count, or past it, does not read.
    for (final rec in [
      [5, 0x00, 0x78],
      [6, 0x21, 0x78],
      [5],
    ]) {
      expect(
        () => readPlain([0, 0, 0, 0, 7, ...rec]),
        throwsA(isA<BadPayload>()),
      );
    }
    expect(
      () => writePlain(const Header(false, 7), [const TextRecord('x', 3, 2)]),
      throwsA(isA<BadPayload>()),
    );
  });

  group('splitting a text', () {
    test('one that fits is one, in either encoding', () {
      expect(splitText('a' * 160, 160), ['a' * 160]);
      // 160 Cyrillic letters are 320 bytes of UTF-8, 160 of the page.
      expect(splitText('ж' * 160, 160), ['ж' * 160]);
    });

    test('ASCII: after the last space near the limit, words whole', () {
      final words = List.filled(40, 'word').join(' ');
      final parts = splitText(words, 160);
      expect(parts.join(), words);
      expect(parts.length, 2);
      expect(parts.first.endsWith('word '), isTrue);
      expect(parts.first.length, lessThanOrEqualTo(159));
      expect(parts.first.length, greaterThan(159 - 5));
    });

    test('no space near the limit: cut at the limit', () {
      final parts = splitText('${'a' * 100} ${'b' * 200}', 160);
      expect(parts.map((p) => p.length), [159, 142]);
      expect(parts.join(), '${'a' * 100} ${'b' * 200}');
    });

    test('Cyrillic: one byte a letter under the page, so 159 a part', () {
      final text = 'ї' * 400;
      final parts = splitText(text, 160);
      expect(parts.map((p) => p.length), [159, 159, 82]);
      expect(parts.every((p) => encode1251(p) != null), isTrue);
    });

    test('Cyrillic with an emoji: the parts before it keep the page', () {
      final text = '${'ї' * 200}🦔';
      final parts = splitText(text, 160);
      expect(parts.join(), text);
      expect(parts.map(textLength), [159, 41 * 2 + 4]);
      expect(encode1251(parts.first), isNotNull);
    });

    test('emoji: never inside one, nor between a flag\'s two halves', () {
      // A flag is two code points of four bytes; 159 is not a multiple
      // of 8, so a cut by bytes alone would split one.
      final text = '🇺🇦' * 30;
      final parts = splitText(text, 160);
      expect(parts.join(), text);
      for (final p in parts) {
        expect(textLength(p), lessThanOrEqualTo(159));
        expect(p.length % 4, 0, reason: 'whole flags only');
      }
      expect(parts.first.length ~/ 4, 19);
    });
  });
}
