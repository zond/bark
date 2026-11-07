// What the app puts in a payload (spec "Phone link", "Mailbox";
// docs/app-layer-2026-11-05.md), with libsodium. bark-app-layer is the
// same in Rust, and its vectors.json is what the tests check this
// against.

import 'dart:convert';
import 'dart:typed_data';

import 'package:characters/characters.dart';
import 'package:crypto/crypto.dart' as hash;
import 'package:sodium/sodium.dart';

import 'link.dart';
import 'words.dart';

/// The format's version, a payload's first byte's high four bits.
const int formatVersion = 1;

/// The longest payload the app writes, so that a mailbox can send it on
/// with [replyOverhead] more.
const int maxAppPayload = maxPayload - replyOverhead;

const int nonceLen = 24;
const int tagLen = 16;
const int keyLen = 32;
const int boxOverhead = 1 + nonceLen + tagLen;
const int introOverhead = 1 + keyLen + tagLen;

/// A reply's count of the items still unsent after its batch.
const int remainingLen = 2;
const int replyOverhead = 1 + tagLen + numberLen + remainingLen;

/// The reply that says nothing is new: the format byte and the tag.
const int nothingNewLen = 1 + tagLen;

/// `sent` counts seconds from 2026-01-01 00:00 UTC.
final DateTime sentEpoch = DateTime.utc(2026);

int sentOf(DateTime t) => t.toUtc().difference(sentEpoch).inSeconds;
DateTime timeOfSent(int sent) => sentEpoch.add(Duration(seconds: sent));

/// Not in this layout, too long, or not opening under the key tried.
class BadPayload implements Exception {
  final String why;
  const BadPayload(this.why);
  @override
  String toString() => why;
}

/// An identity's keys, all from its 128-bit seed.
class Keys {
  final Uint8List seed;
  final KeyPair pair;
  final SecureKey mailbox;
  Keys._(this.seed, this.pair, this.mailbox);

  /// The identity: the public key.
  Uint8List get identity => pair.publicKey;

  factory Keys.fromSeed(Sodium s, List<int> seed) {
    if (seed.length != seedLen) throw const BadPayload('a seed is 16 bytes');
    final master = SecureKey.fromList(
      s,
      s.crypto.genericHash(message: Uint8List.fromList(seed), outLen: keyLen),
    );
    SecureKey sub(int id, String context) => s.crypto.kdf.deriveFromKey(
      masterKey: master,
      context: context,
      subkeyId: BigInt.from(id),
      subkeyLen: keyLen,
    );
    final pair = s.crypto.box.seedKeyPair(sub(1, 'bark-key'));
    final mailbox = sub(2, 'bark-mbx');
    master.dispose();
    return Keys._(Uint8List.fromList(seed), pair, mailbox);
  }
}

const int seedLen = 16;
const int phraseWords = 12;

int _checksum(List<int> seed) => hash.sha256.convert(seed).bytes[0] >> 4;

/// The 12 words of a seed: its 128 bits and 4 of checksum, 11 a word.
List<String> phraseOf(List<int> seed) {
  final bits = [...seed, _checksum(seed) << 4];
  int bit(int i) => (bits[i ~/ 8] >> (7 - i % 8)) & 1;
  return [
    for (var k = 0; k < phraseWords; k++)
      words[List.generate(
        11,
        (j) => bit(k * 11 + j),
      ).fold(0, (n, b) => n << 1 | b)],
  ];
}

/// The seed of 12 words, in any case, or null if they are not a phrase.
Uint8List? seedOfPhrase(String text) {
  final ws = text.toLowerCase().split(RegExp(r'\s+'))
    ..removeWhere((w) => w.isEmpty);
  if (ws.length != phraseWords) return null;
  final bits = List.filled(seedLen + 1, 0);
  for (final (k, w) in ws.indexed) {
    final i = words.indexOf(w);
    if (i < 0) return null;
    for (var j = 0; j < 11; j++) {
      final b = k * 11 + j;
      if ((i >> (10 - j)) & 1 == 1) bits[b ~/ 8] |= 0x80 >> (b % 8);
    }
  }
  final seed = Uint8List.fromList(bits.sublist(0, seedLen));
  return bits[seedLen] >> 4 == _checksum(seed) ? seed : null;
}

enum Form { boxed, introduction, fetch, reply }

int formatByte(Form f) => formatVersion << 4 | f.index;

/// The form of a payload, or null if it is not one this app reads.
Form? formOf(List<int> p) {
  if (p.isEmpty || p[0] >> 4 != formatVersion) return null;
  final f = p[0] & 0xf;
  return f < Form.values.length ? Form.values[f] : null;
}

/// A boxed payload: to a contact, under the pair's key.
Uint8List sealBox(PrecalculatedBox pair, Uint8List nonce, List<int> plain) {
  if (boxOverhead + plain.length > maxAppPayload) {
    throw const BadPayload('too long');
  }
  return Uint8List.fromList([
    formatByte(Form.boxed),
    ...nonce,
    ...pair.easy(message: Uint8List.fromList(plain), nonce: nonce),
  ]);
}

/// The plaintext of a boxed payload, or throws if the pair is not the
/// one.
Uint8List openBox(PrecalculatedBox pair, List<int> payload) {
  if (formOf(payload) != Form.boxed || payload.length < boxOverhead) {
    throw const BadPayload('not boxed');
  }
  try {
    return pair.openEasy(
      cipherText: Uint8List.fromList(payload.sublist(1 + nonceLen)),
      nonce: Uint8List.fromList(payload.sublist(1, 1 + nonceLen)),
    );
  } on SodiumException {
    throw const BadPayload('does not open');
  }
}

/// A first message to `to`, in a sealed box.
Uint8List sealIntro(Sodium s, List<int> to, List<int> plain) {
  if (introOverhead + plain.length > maxAppPayload) {
    throw const BadPayload('too long');
  }
  return Uint8List.fromList([
    formatByte(Form.introduction),
    ...s.crypto.box.seal(
      message: Uint8List.fromList(plain),
      publicKey: Uint8List.fromList(to),
    ),
  ]);
}

Uint8List openIntro(Sodium s, Keys me, List<int> payload) {
  if (formOf(payload) != Form.introduction || payload.length < introOverhead) {
    throw const BadPayload('not an introduction');
  }
  try {
    return s.crypto.box.sealOpen(
      cipherText: Uint8List.fromList(payload.sublist(1)),
      publicKey: me.identity,
      secretKey: me.pair.secretKey,
    );
  } on SodiumException {
    throw const BadPayload('does not open');
  }
}

const int numberLen = 3;
const int maxNumber = (1 << 24) - 1;

/// A fetch: send the newest batch of the items not yet sent to
/// [address].
class Fetch {
  final int sent;
  final Address address;
  const Fetch(this.sent, this.address);

  static const plainLen = 4 + Address.len;
  static const len = boxOverhead + plainLen;

  Uint8List seal(Sodium s, SecureKey key, Uint8List nonce) =>
      Uint8List.fromList([
        formatByte(Form.fetch),
        ...nonce,
        ...s.crypto.secretBox.easy(
          message: Uint8List.fromList([...be(sent, 4), ...address.encode()]),
          nonce: nonce,
          key: key,
        ),
      ]);

  static Fetch open(Sodium s, SecureKey key, List<int> payload) {
    if (formOf(payload) != Form.fetch || payload.length != len) {
      throw const BadPayload('not a fetch');
    }
    final Uint8List b;
    try {
      b = s.crypto.secretBox.openEasy(
        cipherText: Uint8List.fromList(payload.sublist(1 + nonceLen)),
        nonce: Uint8List.fromList(payload.sublist(1, 1 + nonceLen)),
        key: key,
      );
    } on SodiumException {
      throw const BadPayload('does not open');
    }
    final a = b.sublist(4);
    return Fetch(
      fromBe(b.sublist(0, 4)),
      Address(Position.decode(a), a.sublist(Position.wireLen)),
    );
  }
}

/// The most items a fetch is answered with, its batch, so the most places
/// tried: what passes each relay on the way from its bucket as it stands
/// (spec "Mailbox", Why a batch with a count).
const int maxReplies = 32;

/// A reply's nonce, never sent: the fetch's `sent`, the reply's place in
/// the round (0 for nothing new), zeros.
Uint8List replyNonce(int round, int place) => Uint8List.fromList([
  ...be(round, 4),
  ...be(place, numberLen),
  ...List.filled(17, 0),
]);

Uint8List _sealReply(
  Sodium s,
  SecureKey key,
  int round,
  int place,
  List<int> plain,
) => Uint8List.fromList([
  formatByte(Form.reply),
  ...s.crypto.secretBox.easy(
    message: Uint8List.fromList(plain),
    nonce: replyNonce(round, place),
    key: key,
  ),
]);

/// Item [number] at [place] in [round], with [remaining] items still
/// unsent after the round's batch, as the home node sends it.
Uint8List sealReply(
  Sodium s,
  SecureKey key,
  int round,
  int place,
  int number,
  int remaining,
  List<int> item,
) => _sealReply(s, key, round, place, [
  ...be(number, numberLen),
  ...be(remaining, remainingLen),
  ...item,
]);

/// The reply that says the mailbox holds nothing the fetch of [round]
/// asks for.
Uint8List sealNothingNew(Sodium s, SecureKey key, int round) =>
    _sealReply(s, key, round, 0, const []);

/// What a reply says: an item, its number, its place in the round and
/// the items still unsent after the round's batch, or nothing new, and so
/// none unsent, when [item] is null. An empty item is one the home could
/// not send.
class Opened {
  final int number;
  final int place;
  final int remaining;
  final Uint8List? item;
  const Opened(this.number, this.place, this.remaining, this.item);
  bool get nothingNew => item == null;
}

/// A reply to the fetch of [round], or null if it is not one of its.
Opened? openReply(Sodium s, SecureKey key, int round, List<int> payload) {
  if (formOf(payload) != Form.reply || payload.length > maxPayload) {
    return null;
  }
  Uint8List? at(int place) {
    try {
      return s.crypto.secretBox.openEasy(
        cipherText: Uint8List.fromList(payload.sublist(1)),
        nonce: replyNonce(round, place),
        key: key,
      );
    } on SodiumException {
      return null;
    }
  }

  if (payload.length == nothingNewLen) {
    return at(0) == null ? null : const Opened(0, 0, 0, null);
  }
  if (payload.length < replyOverhead) return null;
  for (var place = 1; place <= maxReplies; place++) {
    final p = at(place);
    if (p != null) {
      const head = numberLen + remainingLen;
      return Opened(
        fromBe(p.sublist(0, numberLen)),
        place,
        fromBe(p.sublist(numberLen, head)),
        p.sublist(head),
      );
    }
  }
  return null;
}

List<int> be(int v, int n) => [
  for (var i = n - 1; i >= 0; i--) (v >> (8 * i)) & 0xff,
];
int fromBe(List<int> b) => b.fold(0, (n, x) => n << 8 | x);

/// Whether a message from `sender` to `receiver` sets the direction bit.
bool greater(List<int> sender, List<int> receiver) {
  for (var i = 0; i < sender.length; i++) {
    if (sender[i] != receiver[i]) return sender[i] > receiver[i];
  }
  return false;
}

class Header {
  final bool greater;
  final int sent;
  final Address? address;
  const Header(this.greater, this.sent, [this.address]);
}

sealed class Record {
  const Record();
}

/// A text, or part [part] of [of] of one too long for a message.
class TextRecord extends Record {
  final String text;
  final int part;
  final int of;
  const TextRecord(this.text, [this.part = 1, this.of = 1]);
}

class AckRecord extends Record {
  final int sent;
  const AckRecord(this.sent);
}

class CardRecord extends Record {
  /// The card's bytes.
  final Uint8List card;
  CardRecord(List<int> card) : card = Uint8List.fromList(card);
}

const int _typeUtf8 = 1;
const int _type1251 = 2;
const int _typeAck = 3;
const int _typeCard = 4;
const int _typeUtf8Part = 5;
const int _type1251Part = 6;

/// The most parts a text is split into: a part's number and the count
/// share one byte, four bits each.
const int maxParts = 16;

/// A plaintext: the header, then the records, a text last.
Uint8List writePlain(Header h, List<Record> records) {
  final b = <int>[
    (h.address != null ? 1 : 0) | (h.greater ? 2 : 0),
    ...be(h.sent, 4),
    ...?h.address?.encode(),
  ];
  for (final (i, r) in records.indexed) {
    switch (r) {
      case TextRecord(:final text, :final part, :final of):
        if (i != records.length - 1) throw const BadPayload('text not last');
        final w = encode1251(text);
        if (of == 1) {
          b.add(w == null ? _typeUtf8 : _type1251);
        } else if (of <= maxParts && part >= 1 && part <= of) {
          b.addAll([
            w == null ? _typeUtf8Part : _type1251Part,
            (part - 1) << 4 | (of - 1),
          ]);
        } else {
          throw const BadPayload('no such part');
        }
        b.addAll(w ?? utf8.encode(text));
      case AckRecord(:final sent):
        b.addAll([_typeAck, ...be(sent, 4)]);
      case CardRecord(:final card):
        b.addAll([_typeCard, card.length, ...card]);
    }
  }
  return Uint8List.fromList(b);
}

/// The header and records of a plaintext; throws if it does not read.
(Header, List<Record>) readPlain(List<int> p) {
  if (p.length < 5 || p[0] & ~3 != 0) throw const BadPayload('no header');
  var at = 5;
  Address? a;
  if (p[0] & 1 != 0) {
    if (p.length < at + Address.len) throw const BadPayload('no address');
    final ab = p.sublist(at, at + Address.len);
    a = Address(Position.decode(ab), ab.sublist(Position.wireLen));
    at += Address.len;
  }
  final records = <Record>[];
  while (at < p.length) {
    final t = p[at++];
    switch (t) {
      case _typeUtf8 || _type1251 || _typeUtf8Part || _type1251Part:
        var (part, of) = (1, 1);
        if (t == _typeUtf8Part || t == _type1251Part) {
          if (at == p.length) throw const BadPayload('no part');
          (part, of) = ((p[at] >> 4) + 1, (p[at] & 15) + 1);
          if (of < 2 || part > of) throw const BadPayload('no such part');
          at++;
        }
        final body = p.sublist(at);
        final String text;
        if (t == _typeUtf8 || t == _typeUtf8Part) {
          try {
            text = utf8.decode(body);
          } on FormatException {
            throw const BadPayload('text not UTF-8');
          }
        } else {
          text =
              decode1251(body) ?? (throw const BadPayload('not windows-1251'));
        }
        records.add(TextRecord(text, part, of));
        at = p.length;
      case _typeAck when at + 4 <= p.length:
        records.add(AckRecord(fromBe(p.sublist(at, at + 4))));
        at += 4;
      case _typeCard when at < p.length && at + 1 + p[at] <= p.length:
        records.add(CardRecord(p.sublist(at + 1, at + 1 + p[at])));
        at += 1 + p[at];
      default:
        throw const BadPayload('a record this app does not read');
    }
  }
  return (Header(p[0] & 2 != 0, fromBe(p.sublist(1, 5)), a), records);
}

/// windows-1251's bytes 0x80 to 0xFF, 0 where it has none (0x98).
const List<int> _high = [
  0x0402,
  0x0403,
  0x201A,
  0x0453,
  0x201E,
  0x2026,
  0x2020,
  0x2021,
  0x20AC,
  0x2030,
  0x0409,
  0x2039,
  0x040A,
  0x040C,
  0x040B,
  0x040F,
  0x0452,
  0x2018,
  0x2019,
  0x201C,
  0x201D,
  0x2022,
  0x2013,
  0x2014,
  0,
  0x2122,
  0x0459,
  0x203A,
  0x045A,
  0x045C,
  0x045B,
  0x045F,
  0x00A0,
  0x040E,
  0x045E,
  0x0408,
  0x00A4,
  0x0490,
  0x00A6,
  0x00A7,
  0x0401,
  0x00A9,
  0x0404,
  0x00AB,
  0x00AC,
  0x00AD,
  0x00AE,
  0x0407,
  0x00B0,
  0x00B1,
  0x0406,
  0x0456,
  0x0491,
  0x00B5,
  0x00B6,
  0x00B7,
  0x0451,
  0x2116,
  0x0454,
  0x00BB,
  0x0458,
  0x0405,
  0x0455,
  0x0457,
  0x0410,
  0x0411,
  0x0412,
  0x0413,
  0x0414,
  0x0415,
  0x0416,
  0x0417,
  0x0418,
  0x0419,
  0x041A,
  0x041B,
  0x041C,
  0x041D,
  0x041E,
  0x041F,
  0x0420,
  0x0421,
  0x0422,
  0x0423,
  0x0424,
  0x0425,
  0x0426,
  0x0427,
  0x0428,
  0x0429,
  0x042A,
  0x042B,
  0x042C,
  0x042D,
  0x042E,
  0x042F,
  0x0430,
  0x0431,
  0x0432,
  0x0433,
  0x0434,
  0x0435,
  0x0436,
  0x0437,
  0x0438,
  0x0439,
  0x043A,
  0x043B,
  0x043C,
  0x043D,
  0x043E,
  0x043F,
  0x0440,
  0x0441,
  0x0442,
  0x0443,
  0x0444,
  0x0445,
  0x0446,
  0x0447,
  0x0448,
  0x0449,
  0x044A,
  0x044B,
  0x044C,
  0x044D,
  0x044E,
  0x044F,
];

/// A text's bytes in a record: windows-1251 if the page holds it, else
/// UTF-8, as [writePlain] chooses.
int textLength(String text) =>
    encode1251(text)?.length ?? utf8.encode(text).length;

/// `text` as one text, if its bytes fit `room`, else as parts whose bytes
/// fit `room` less the byte that numbers them; the caller refuses more
/// than [maxParts]. A part ends between two characters as a person sees
/// them, after the last space in its last quarter if there is one, so that
/// words of ordinary length stay whole and no part is much short of the
/// room. Each part takes the encoding that holds it, so a Cyrillic part
/// before an emoji keeps one byte a letter.
List<String> splitText(String text, int room) {
  if (textLength(text) <= room) return [text];
  final cs = text.characters.toList();
  final parts = <String>[];
  var i = 0;
  while (i < cs.length) {
    // The longest run that fits: windows-1251's bytes while the page
    // holds every character, else UTF-8's, which only grow.
    var (page, n1251, nUtf8, end) = (true, 0, 0, i);
    while (end < cs.length) {
      final w = encode1251(cs[end]);
      page = page && w != null;
      final b1251 = n1251 + (w?.length ?? 0);
      final bUtf8 = nUtf8 + utf8.encode(cs[end]).length;
      if ((page ? b1251 : bUtf8) > room - 1) break;
      (n1251, nUtf8) = (b1251, bUtf8);
      end++;
    }
    if (end == i) throw const BadPayload('a character too long to send');
    if (end < cs.length) {
      for (var j = end - 1; j >= end - (end - i) ~/ 4 && j > i; j--) {
        if (cs[j].trim().isEmpty) {
          end = j + 1;
          break;
        }
      }
    }
    parts.add(cs.sublist(i, end).join());
    i = end;
  }
  return parts;
}

/// The text in windows-1251, or null if the page lacks a character.
Uint8List? encode1251(String text) {
  final out = <int>[];
  for (final r in text.runes) {
    if (r < 0x80) {
      out.add(r);
      continue;
    }
    final i = _high.indexOf(r);
    if (i < 0) return null;
    out.add(0x80 + i);
  }
  return Uint8List.fromList(out);
}

String? decode1251(List<int> b) {
  final rs = <int>[];
  for (final x in b) {
    final r = x < 0x80 ? x : _high[x - 0x80];
    if (r == 0) return null;
    rs.add(r);
  }
  return String.fromCharCodes(rs);
}
