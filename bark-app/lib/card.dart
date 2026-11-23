// A card: what two people exchange so that each can reach the other,
// shown and scanned as a QR code (spec "Phone link"): an identity, the
// home node's address if the person has one, the node the phone is linked
// to, or was last, and when, when the home was set or cleared, and a name.
// A card without a home is temporary.

import 'dart:convert';
import 'dart:typed_data';

import 'app_layer.dart';
import 'l10n.dart';
import 'link.dart';

/// A value that is not a card this app reads. Its words reach the screen,
/// so they are in the language spoken.
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

  /// The name its person chose.
  final String name;

  /// The newest payload version their app reads, in the flags' high four
  /// bits (spec "Phone link", The payload, the card): a sender writes
  /// them none newer. This app's is [formatVersion].
  final int reads;

  PersonCard(
    List<int> identity,
    this.home,
    this.time,
    this.name, {
    this.now,
    this.nowAt = 0,
    this.reads = formatVersion,
  }) : identity = Uint8List.fromList(identity);

  /// The identity in hex: what a contact and a conversation are known by.
  String get key => hex(identity);

  /// No home: they can be reached only at the node they last used, which
  /// keeps their mail only while they come back to it.
  bool get temporary => home == null;

  Uint8List encode() {
    final n = utf8.encode(name);
    if (n.length > maxName) throw BadCard(tr.cardNameTooLong);
    final h = home, w = now;
    return Uint8List.fromList([
      version,
      reads << 4 | (h != null ? _flagHome : 0) | (w != null ? _flagNow : 0),
      ...identity,
      ...be(time, 4),
      ...?h?.encode(),
      if (w != null) ...[...w.encode(), ...be(nowAt, 4)],
      ...n,
    ]);
  }

  static PersonCard decode(List<int> b) {
    if (b.isEmpty) throw BadCard(tr.cardNot);
    if (b[0] > version) {
      throw BadCard(tr.cardNewer(b[0]));
    }
    if (b[0] < version) {
      throw BadCard(tr.cardOlder(b[0]));
    }
    if (b.length < fixedLen || b[1] & 0x0f & ~(_flagHome | _flagNow) != 0) {
      throw BadCard(tr.cardNot);
    }
    var at = fixedLen;
    Address? address(int flag) {
      if (b[1] & flag == 0) return null;
      if (b.length < at + Address.len) throw BadCard(tr.cardNot);
      final a = b.sublist(at, at += Address.len);
      return Address(Position.decode(a), a.sublist(Position.wireLen));
    }

    final home = address(_flagHome);
    final now = address(_flagNow);
    var nowAt = 0;
    if (now != null) {
      if (b.length < at + 4) throw BadCard(tr.cardNot);
      nowAt = fromBe(b.sublist(at, at += 4));
    }
    if (b.length > at + maxName) throw BadCard(tr.cardNot);
    try {
      return PersonCard(
        b.sublist(2, 2 + keyLen),
        home,
        fromBe(b.sublist(2 + keyLen, fixedLen)),
        utf8.decode(b.sublist(at)),
        now: now,
        nowAt: nowAt,
        reads: b[1] >> 4,
      );
    } on FormatException {
      throw BadCard(tr.cardNameNotUtf8);
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
    if (!t.startsWith(prefix)) throw BadCard(tr.cardNot);
    final body = t.substring(prefix.length);
    final n = body.length * 5 ~/ 8;
    if (body.length * 5 - n * 8 >= 5) throw BadCard(tr.cardNot);
    final b = Uint8List(n);
    for (final (k, c) in body.split('').indexed) {
      final v = _base32.indexOf(c);
      if (v < 0) throw BadCard(tr.cardNot);
      for (var j = 0; j < 5; j++) {
        final i = k * 5 + j;
        if ((v >> (4 - j)) & 1 == 0) continue;
        if (i >= n * 8) throw BadCard(tr.cardNot);
        b[i ~/ 8] |= 0x80 >> (i % 8);
      }
    }
    return decode(b);
  }
}
