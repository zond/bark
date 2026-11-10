// A card: what two people exchange so that each can reach the other,
// shown and scanned as a QR code (spec "Phone link"): an identity, the
// home node's address if the person has one, the node the phone is linked
// to, or was last, and when, when the home was set or cleared, and a name.
// A card without a home is temporary.

import 'dart:convert';
import 'dart:typed_data';

import 'app_layer.dart';
import 'link.dart';

/// A value that is not a card this app reads.
class BadCard implements Exception {
  final String why;
  const BadCard(this.why);
  @override
  String toString() => why;
}

class PersonCard {
  static const version = 3;

  /// Bytes before the addresses: the shortest card.
  static const fixedLen = 1 + 1 + keyLen + 4;
  static const _flagHome = 1;
  static const _flagNow = 2;
  static const maxName = 32;
  static const prefix = 'BARK:';
  static const _base32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

  final Uint8List identity;

  /// Their home node's address, whose mailbox keeps their mail.
  final Address? home;

  /// The node their phone is linked to, or was last, their own or a
  /// public one: where to reach them now; none when it is the home. A
  /// public node keeps a guest's mail by its recipient tag (spec "Phone
  /// link", Public), so a text there waits for them.
  final Address? now;

  /// When the phone was last linked to [now], as `sent`: seconds since
  /// 2026-01-01 00:00 UTC, as [time], which 4 bytes hold for 136 years.
  final int nowAt;

  /// When the home was set or cleared, as `sent`: a later card replaces
  /// an earlier one, also one that drops the home.
  final int time;

  /// The name its owner chose.
  final String name;

  PersonCard(
    List<int> identity,
    this.home,
    this.time,
    this.name, {
    this.now,
    this.nowAt = 0,
  }) : identity = Uint8List.fromList(identity);

  /// The identity in hex: what a contact and a conversation are known by.
  String get key => hex(identity);

  /// No home: they can be reached only at the node they last used, which
  /// keeps their mail only while they come back to it.
  bool get temporary => home == null;

  Uint8List encode() {
    final n = utf8.encode(name);
    if (n.length > maxName) throw const BadCard('The name is too long');
    final h = home, w = now;
    return Uint8List.fromList([
      version,
      (h != null ? _flagHome : 0) | (w != null ? _flagNow : 0),
      ...identity,
      ...be(time, 4),
      ...?h?.encode(),
      if (w != null) ...[...w.encode(), ...be(nowAt, 4)],
      ...n,
    ]);
  }

  static PersonCard decode(List<int> b) {
    if (b.isEmpty) throw const BadCard('Not a card');
    if (b[0] > version) {
      throw BadCard('A card from a newer app (card version ${b[0]})');
    }
    if (b[0] < version) {
      throw BadCard('A card from an older app (card version ${b[0]})');
    }
    if (b.length < fixedLen || b[1] & ~(_flagHome | _flagNow) != 0) {
      throw const BadCard('Not a card');
    }
    var at = fixedLen;
    Address? address(int flag) {
      if (b[1] & flag == 0) return null;
      if (b.length < at + Address.len) throw const BadCard('Not a card');
      final a = b.sublist(at, at += Address.len);
      return Address(Position.decode(a), a.sublist(Position.wireLen));
    }

    final home = address(_flagHome);
    final now = address(_flagNow);
    var nowAt = 0;
    if (now != null) {
      if (b.length < at + 4) throw const BadCard('Not a card');
      nowAt = fromBe(b.sublist(at, at += 4));
    }
    if (b.length > at + maxName) throw const BadCard('Not a card');
    try {
      return PersonCard(
        b.sublist(2, 2 + keyLen),
        home,
        fromBe(b.sublist(2 + keyLen, fixedLen)),
        utf8.decode(b.sublist(at)),
        now: now,
        nowAt: nowAt,
      );
    } on FormatException {
      throw const BadCard('Not a card: the name is not UTF-8');
    }
  }

  /// `BARK:` and the bytes in base32 without padding: every character is
  /// in the QR code's alphanumeric set.
  String toText() {
    final b = encode();
    final out = StringBuffer(prefix);
    for (var k = 0; k * 5 < b.length * 8; k++) {
      var v = 0;
      for (var j = 0; j < 5; j++) {
        final i = k * 5 + j;
        v = v << 1 | (i < b.length * 8 ? (b[i ~/ 8] >> (7 - i % 8)) & 1 : 0);
      }
      out.write(_base32[v]);
    }
    return out.toString();
  }

  static PersonCard parse(String text) {
    final t = text.trim().toUpperCase();
    if (!t.startsWith(prefix)) throw const BadCard('Not a card');
    final body = t.substring(prefix.length);
    final n = body.length * 5 ~/ 8;
    if (body.length * 5 - n * 8 >= 5) throw const BadCard('Not a card');
    final b = Uint8List(n);
    for (final (k, c) in body.split('').indexed) {
      final v = _base32.indexOf(c);
      if (v < 0) throw const BadCard('Not a card');
      for (var j = 0; j < 5; j++) {
        final i = k * 5 + j;
        if ((v >> (4 - j)) & 1 == 0) continue;
        if (i >= n * 8) throw const BadCard('Not a card');
        b[i ~/ 8] |= 0x80 >> (i % 8);
      }
    }
    return decode(b);
  }
}
