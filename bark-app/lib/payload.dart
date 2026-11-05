// The prototype's payload: a random nonce, then UTF-8 text. Not
// encrypted; end-to-end encryption is open item O1 (README.md).

import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'link.dart';

/// 96 bits: the nonce of the common authenticated ciphers (RFC 8439,
/// NIST SP 800-38D), so encryption can keep this layout. Random, it
/// makes two payloads equal with probability 2⁻⁹⁶, which keeps every
/// message's ID its own (spec "Phone link", "The payload").
const int nonceLen = 12;

/// The longest text, in UTF-8 bytes.
const int maxTextBytes = maxPayload - nonceLen;

final Random _random = Random.secure();

Uint8List encodeText(String text, {Random? random}) {
  final body = utf8.encode(text);
  if (body.length > maxTextBytes) throw const Malformed('text too long');
  final r = random ?? _random;
  return Uint8List.fromList([
    for (var i = 0; i < nonceLen; i++) r.nextInt(256),
    ...body,
  ]);
}

/// The text of a payload, or null if it is not in this format.
String? decodeText(List<int> payload) {
  if (payload.length < nonceLen) return null;
  try {
    return utf8.decode(payload.sublist(nonceLen));
  } on FormatException {
    return null;
  }
}
