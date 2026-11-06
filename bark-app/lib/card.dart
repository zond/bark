// A card: what two people exchange so that each can reach the other,
// shown and scanned as a QR code (spec "Phone link"): an identity, an
// address and whether it is a home node's, the address's time and a name.

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
  static const version = 2;
  static const fixedLen = 1 + 1 + keyLen + Address.len + 4;
  static const _flagHome = 1;
  static const maxName = 32;
  static const prefix = 'BARK:';
  static const _base32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

  final Uint8List identity;

  /// The home node's address, or without one the person's node's
  /// current one.
  final Address address;

  /// The address is their home node's, whose mailbox keeps their mail;
  /// else it is where their node was.
  final bool home;

  /// When the address became theirs, as `sent`: a later card replaces
  /// an earlier one's address.
  final int time;

  /// The name its owner chose.
  final String name;

  PersonCard(
    List<int> identity,
    this.address,
    this.time,
    this.name, {
    required this.home,
  }) : identity = Uint8List.fromList(identity);

  /// The identity in hex: what a contact and a conversation are known by.
  String get key => hex(identity);

  Uint8List encode() {
    final n = utf8.encode(name);
    if (n.length > maxName) throw const BadCard('The name is too long');
    return Uint8List.fromList([
      version,
      home ? _flagHome : 0,
      ...identity,
      ...address.encode(),
      ...be(time, 4),
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
    if (b.length < fixedLen ||
        b.length > fixedLen + maxName ||
        b[1] & ~_flagHome != 0) {
      throw const BadCard('Not a card');
    }
    final a = b.sublist(2 + keyLen, 2 + keyLen + Address.len);
    try {
      return PersonCard(
        b.sublist(2, 2 + keyLen),
        Address(Position.decode(a), a.sublist(Position.wireLen)),
        fromBe(b.sublist(fixedLen - 4, fixedLen)),
        utf8.decode(b.sublist(fixedLen)),
        home: b[1] & _flagHome != 0,
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
