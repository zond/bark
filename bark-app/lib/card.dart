// A card: what two people exchange so that each can reach the other,
// shown and scanned as a QR code. It will grow an identity key, so it
// starts with a version.

import 'dart:convert';
import 'dart:typed_data';

import 'link.dart';

/// A value that is not a card this app reads.
class BadCard implements Exception {
  final String why;
  const BadCard(this.why);
  @override
  String toString() => why;
}

class PersonCard {
  /// The card's layout: version(1) address(13) name(UTF-8, the rest).
  static const version = 1;

  final Address address;

  /// The name its owner chose.
  final String name;

  PersonCard(this.address, this.name);

  Uint8List encode() =>
      Uint8List.fromList([version, ...address.encode(), ...utf8.encode(name)]);

  static PersonCard decode(List<int> b) {
    if (b.isEmpty) throw const BadCard('Not a card');
    if (b[0] > version) {
      throw BadCard('A card from a newer app (card version ${b[0]})');
    }
    if (b[0] != version || b.length < 1 + Address.len) {
      throw const BadCard('Not a card');
    }
    final a = b.sublist(1, 1 + Address.len);
    try {
      return PersonCard(
        Address(Position.decode(a), a.sublist(Position.wireLen)),
        utf8.decode(b.sublist(1 + Address.len)),
      );
    } on FormatException {
      throw const BadCard('Not a card: the name is not UTF-8');
    }
  }

  /// The card as text, in the QR code and to paste: base64url, since a
  /// scanner hands over a QR code's content as text and may mangle bytes
  /// that are not.
  String toText() => base64Url.encode(encode());

  static PersonCard parse(String text) {
    final List<int> b;
    try {
      b = base64Url.decode(text.trim());
    } on FormatException {
      throw const BadCard('Not a card');
    }
    return decode(b);
  }
}
