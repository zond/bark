// What the app puts in a payload (spec "Phone link", "Mailbox";
// docs/app-layer-2026-11-05.md), with libsodium. bark-app-layer is the
// same in Rust, and its vectors.json is what the tests check this
// against.

import 'dart:convert';
import 'dart:ffi' show UnsignedChar;
import 'dart:typed_data';

import 'package:characters/characters.dart';
import 'package:crypto/crypto.dart' as hash;
import 'package:sodium/sodium.dart';
import 'package:sodium/sodium.ffi.dart' show LibSodiumFFI, SodiumPointer;

import 'link.dart';
import 'words.dart';

/// The format's version, a payload's first byte's high four bits: 2
/// since the recipient tag.
const int formatVersion = 2;

/// The longest payload the app writes, so that a mailbox can send it on
/// with [replyOverhead] more.
const int maxAppPayload = maxPayload - replyOverhead;

/// XSalsa20's nonce, which every form's stream takes.
const int nonceLen = 24;
const int tagLen = 16;
const int keyLen = 32;

/// `siv`: a boxed payload's or a fetch's nonce and its proof in one.
const int sivLen = 16;

/// Bytes before a form's own: the format byte and the recipient tag.
const int headLen = 2;

/// What a boxed payload or a fetch adds to its plaintext.
const int boxOverhead = headLen + sivLen;
const int introOverhead = headLen + keyLen + tagLen;

/// A reply's place in its round, in the clear before the item it carries.
const int placeLen = 1;

/// What a reply adds to the item it carries: its own format byte and
/// recipient tag stand for the item's, and the place is added.
const int replyOverhead = placeLen;

/// The summary's counts, the round's items and those still unsent after
/// them, and its flags.
const int itemsLen = 1;
const int remainingLen = 2;
const int flagsLen = 1;

/// The shortest summary: the format byte, the recipient tag, the tag, the
/// two counts and the flags.
const int summaryLen = headLen + tagLen + itemsLen + remainingLen + flagsLen;

/// `crypto_generichash`'s shortest output, of which the recipient tag is
/// the first byte.
const int _tagHashLen = 16;

/// The recipient tag of `identity` at the node `nodeId`: the first byte
/// of `crypto_generichash` of the node ID keyed with the identity (spec
/// "Phone link", Recipient tag). A public node hands a payload to the
/// phones whose tag it carries.
int recipientTag(Sodium s, List<int> identity, List<int> nodeId) {
  final key = SecureKey.fromList(s, Uint8List.fromList(identity));
  try {
    return s.crypto.genericHash(
      message: Uint8List.fromList(nodeId),
      outLen: _tagHashLen,
      key: key,
    )[0];
  } finally {
    key.dispose();
  }
}

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

  /// Frees the secret keys, once the identity is replaced; any use of
  /// them after is a use of freed memory.
  void dispose() {
    if (disposed) return;
    disposed = true;
    pair.secretKey.dispose();
    mailbox.dispose();
  }

  bool disposed = false;

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
    final boxSeed = sub(1, 'bark-key');
    final KeyPair pair;
    try {
      pair = s.crypto.box.seedKeyPair(boxSeed);
    } finally {
      boxSeed.dispose();
    }
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

/// The forms, by number: the mailbox sends a round's summary, then each
/// boxed payload or introduction it kept as a form of its own.
enum Form {
  boxed,
  introduction,
  fetch,
  summary,
  boxedItem,
  introductionItem;

  bool get isReply => index >= summary.index;
}

int formatByte(Form f) => formatVersion << 4 | f.index;

/// The form of a payload, or null if it is not one this app reads.
Form? formOf(List<int> p) {
  if (p.isEmpty || p[0] >> 4 != formatVersion) return null;
  final f = p[0] & 0xf;
  return f < Form.values.length ? Form.values[f] : null;
}

/// XSalsa20 over `m` under `key` with `nonce`, as
/// `crypto_secretbox_detached` encrypts, its tag dropped: the keystream
/// from its 33rd byte. Its own inverse.
Uint8List stream(Sodium s, SecureKey key, Uint8List nonce, List<int> m) => s
    .crypto
    .secretBox
    .detached(message: Uint8List.fromList(m), nonce: nonce, key: key)
    .cipherText;

/// The two keys a boxed payload or a fetch is made with, derived from one
/// shared key: `siv` is hashed under [mac], the stream runs under [enc]
/// (spec "Phone link", The payload).
class SivKeys {
  final Sodium s;
  final SecureKey mac;
  final SecureKey enc;
  SivKeys._(this.s, this.mac, this.enc);

  /// The keys derived from `shared`: a pair's, or the mailbox key.
  factory SivKeys.of(Sodium s, SecureKey shared) {
    SecureKey sub(int id, String context) => s.crypto.kdf.deriveFromKey(
      masterKey: shared,
      context: context,
      subkeyId: BigInt.from(id),
      subkeyLen: keyLen,
    );
    return SivKeys._(s, sub(1, 'bark-mac'), sub(2, 'bark-enc'));
  }

  /// The keys of the pair of `them` and the person's secret key, from
  /// `crypto_box_beforenm`: the same both ways.
  factory SivKeys.pair(Sodium s, List<int> them, SecureKey secret) {
    final shared = _beforenm(s, them, secret);
    try {
      return SivKeys.of(s, shared);
    } finally {
      shared.dispose();
    }
  }

  Uint8List _siv(int format, List<int> bound, List<int> plain) =>
      s.crypto.genericHash(
        message: Uint8List.fromList([format, ...bound, ...plain]),
        outLen: sivLen,
        key: mac,
      );

  static Uint8List _nonce(List<int> siv) =>
      Uint8List.fromList([...siv, ...List.filled(nonceLen - sivLen, 0)]);

  /// `plain` as a payload of `form` for the `recipient` tag: the format
  /// byte, the tag, `siv`, the ciphertext. `bound` is hashed into `siv`
  /// and not sent: what the receiver knows itself, so that the payload
  /// opens there alone.
  Uint8List seal(
    Form form,
    int recipient,
    List<int> plain, {
    List<int> bound = const [],
  }) {
    final siv = _siv(formatByte(form), bound, plain);
    return Uint8List.fromList([
      formatByte(form),
      recipient,
      ...siv,
      ...stream(s, enc, _nonce(siv), plain),
    ]);
  }

  /// The plaintext of `payload`, sealed with `bound`, which is given only
  /// once its `siv` checks; null if it does not.
  Uint8List? open(List<int> payload, {List<int> bound = const []}) {
    if (payload.length < boxOverhead) return null;
    final siv = payload.sublist(headLen, boxOverhead);
    final plain = stream(s, enc, _nonce(siv), payload.sublist(boxOverhead));
    final want = _siv(payload[0], bound, plain);
    var d = 0;
    for (var i = 0; i < sivLen; i++) {
      d |= want[i] ^ siv[i];
    }
    if (d == 0) return plain;
    plain.fillRange(0, plain.length, 0);
    return null;
  }

  void dispose() {
    mac.dispose();
    enc.dispose();
  }
}

/// `crypto_box_beforenm` of `them` and `secret`: the sodium package keeps
/// it inside its precalculated box, so it is called through the package's
/// binding of libsodium itself.
SecureKey _beforenm(Sodium s, List<int> them, SecureKey secret) {
  const lib = LibSodiumFFI();
  final k = SodiumPointer<UnsignedChar>.alloc(lib, count: keyLen);
  final pk = SodiumPointer<UnsignedChar>.alloc(lib, count: keyLen);
  final sk = SodiumPointer<UnsignedChar>.alloc(lib, count: keyLen);
  try {
    if (them.length != keyLen) throw const BadPayload('not a public key');
    pk.fill(them);
    secret.runUnlockedSync(sk.fill);
    if (lib.crypto_box_beforenm(k.ptr, pk.ptr, sk.ptr) != 0) {
      throw const BadPayload('no shared key with that public key');
    }
    return SecureKey.fromList(s, Uint8List.fromList(k.asListView()));
  } finally {
    sk.zeroMemory();
    k.zeroMemory();
    k.dispose();
    pk.dispose();
    sk.dispose();
  }
}

/// A boxed payload: to a contact, under the pair's keys, with their
/// `recipient` tag at the node it goes to.
Uint8List sealBox(SivKeys pair, int recipient, List<int> plain) {
  if (boxOverhead + plain.length > maxAppPayload) {
    throw const BadPayload('too long');
  }
  return pair.seal(Form.boxed, recipient, plain);
}

/// The plaintext of a boxed payload, or throws if the pair is not the
/// one.
Uint8List openBox(SivKeys pair, List<int> payload) {
  if (formOf(payload) != Form.boxed || payload.length > maxPayload) {
    throw const BadPayload('not boxed');
  }
  return pair.open(payload) ?? (throw const BadPayload('does not open'));
}

/// A first message to `to`, in a sealed box, with their `recipient` tag
/// at the node it goes to.
Uint8List sealIntro(Sodium s, List<int> to, int recipient, List<int> plain) {
  if (introOverhead + plain.length > maxAppPayload) {
    throw const BadPayload('too long');
  }
  return Uint8List.fromList([
    formatByte(Form.introduction),
    recipient,
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
      cipherText: Uint8List.fromList(payload.sublist(headLen)),
      publicKey: me.identity,
      secretKey: me.pair.secretKey,
    );
  } on SodiumException {
    throw const BadPayload('does not open');
  }
}

/// What the owner, or for [LeaveCommand] anyone homed, asks their home
/// node on a fetch (spec "Mailbox", The owner's commands).
sealed class Command {
  const Command();

  Uint8List encode();

  /// The command `b` holds whole.
  static Command decode(List<int> b) {
    if (b.isEmpty) throw const BadPayload('no command');
    int number() => b.length == 3
        ? fromBe(b.sublist(1))
        : throw const BadPayload('not a command');
    final add = b.length >= 2 + keyLen && b.length <= AddCommand.maxLen;
    return switch (b[0]) {
      AddCommand.op when add => AddCommand(
        b.sublist(1, 1 + keyLen),
        b[1 + keyLen],
        _utf8(b.sublist(2 + keyLen)),
      ),
      RemoveCommand.op => RemoveCommand(number()),
      ListCommand.op when b.length == 1 => const ListCommand(),
      HandOverCommand.op => HandOverCommand(number()),
      LeaveCommand.op when b.length == 1 => const LeaveCommand(),
      _ => throw const BadPayload('not a command'),
    };
  }
}

String _utf8(List<int> b) {
  try {
    return utf8.decode(b);
  } on FormatException {
    throw const BadPayload('not UTF-8');
  }
}

/// A record for a person with no phone at the node: their mailbox key,
/// their recipient tag there and their name, cut to [nameMax] bytes.
class AddCommand extends Command {
  static const op = 1;
  static const maxLen = 2 + keyLen + nameMax;
  final Uint8List key;
  final int tag;
  final String name;
  AddCommand(List<int> key, this.tag, this.name)
    : key = Uint8List.fromList(key) {
    if (key.length != keyLen) throw const BadPayload('a key is 32 bytes');
  }
  @override
  Uint8List encode() =>
      Uint8List.fromList([op, ...key, tag, ...nameBytes(name)]);
}

/// The record of that number loses its home.
class RemoveCommand extends Command {
  static const op = 2;
  final int number;
  const RemoveCommand(this.number);
  @override
  Uint8List encode() => Uint8List.fromList([op, ...be(number, 2)]);
}

/// Every record, in the summary.
class ListCommand extends Command {
  static const op = 3;
  const ListCommand();
  @override
  Uint8List encode() => Uint8List.fromList([op]);
}

/// The key of the record of that number, which holds a home, is the
/// owner's.
class HandOverCommand extends Command {
  static const op = 4;
  final int number;
  const HandOverCommand(this.number);
  @override
  Uint8List encode() => Uint8List.fromList([op, ...be(number, 2)]);
}

/// The node forgets the sender's key.
class LeaveCommand extends Command {
  static const op = 5;
  const LeaveCommand();
  @override
  Uint8List encode() => Uint8List.fromList([op]);
}

/// A fetch: send the newest batch of the items not yet sent to
/// [address], for the requester's [recipient] tag at that address's node,
/// which the home cannot make as it does not know the identity; it may
/// end in a [command].
class Fetch {
  final int sent;
  final Address address;
  final int recipient;
  final Command? command;
  const Fetch(this.sent, this.address, this.recipient, [this.command]);

  static const plainLen = 4 + Address.len + 1;
  static const len = boxOverhead + plainLen;

  /// The fetch as a payload under the mailbox key's keys, for the home
  /// node of ID `home`, with `tag`, the requester's recipient tag there.
  /// The home's ID is hashed into `siv`, so that no other node holding the
  /// key, an earlier home, can take it.
  Uint8List seal(Sodium s, SecureKey key, List<int> home, int tag) {
    final keys = SivKeys.of(s, key);
    try {
      return keys.seal(Form.fetch, tag, [
        ...be(sent, 4),
        ...address.encode(),
        recipient,
        ...?command?.encode(),
      ], bound: home);
    } finally {
      keys.dispose();
    }
  }

  /// The fetch in `payload` under the mailbox `key`, at the node of ID
  /// `me`; one whose command does not read is no fetch.
  static Fetch open(Sodium s, SecureKey key, List<int> me, List<int> payload) {
    if (formOf(payload) != Form.fetch ||
        payload.length < len ||
        payload.length > len + AddCommand.maxLen) {
      throw const BadPayload('not a fetch');
    }
    final keys = SivKeys.of(s, key);
    final Uint8List b;
    try {
      b =
          keys.open(payload, bound: me) ??
          (throw const BadPayload('does not open'));
    } finally {
      keys.dispose();
    }
    final a = b.sublist(4, 4 + Address.len);
    return Fetch(
      fromBe(b.sublist(0, 4)),
      Address(Position.decode(a), a.sublist(Position.wireLen)),
      b[4 + Address.len],
      b.length == plainLen ? null : Command.decode(b.sublist(plainLen)),
    );
  }
}

/// The most items a fetch is answered with, its batch: what passes each
/// relay on the way from its bucket as it stands (spec "Mailbox", The
/// answer).
const int maxReplies = 32;

/// The key a mailbox's replies are under, derived from the mailbox key as
/// the fetch's two are, so that the mailbox key itself encrypts nothing.
SecureKey replyKey(Sodium s, SecureKey mailbox) => s.crypto.kdf.deriveFromKey(
  masterKey: mailbox,
  context: 'bark-rpl',
  subkeyId: BigInt.from(3),
  subkeyLen: keyLen,
);

/// `f` of the reply key of `mailbox`, which is wiped after.
T _underReplyKey<T>(Sodium s, SecureKey mailbox, T Function(SecureKey) f) {
  final k = replyKey(s, mailbox);
  try {
    return f(k);
  } finally {
    k.dispose();
  }
}

/// A reply's nonce, never sent: the fetch's `sent`, the reply's place in
/// the round (0 for the summary), zeros.
Uint8List replyNonce(int round, int place) => Uint8List.fromList([
  ...be(round, 4),
  ...be(place, placeLen),
  ...List.filled(nonceLen - 4 - placeLen, 0),
]);

/// What a command did, or that the sender's home was removed, as its
/// summary says (spec "Mailbox", The owner's commands), by its code.
enum Outcome {
  /// Done; for a list, the records follow.
  ok,

  /// The sender's key is not the owner's; nothing changed.
  notAllowed,

  /// An add, and no record is free.
  full,

  /// An add of a key a record holds; nothing changed.
  already,

  /// No record of that number holds a home.
  noSuchHome,

  /// The sender's home there was removed; nothing else was done
  /// (spec "Mailbox", The answer).
  removed,
}

/// What a round's summary says: how many items it carries, and how many
/// stay unsent at home after them, no items being nothing new; whether
/// the requester's key is the owner's; and what the fetch's command did,
/// with the records after an ok to a list.
class RoundSummary {
  final int items;
  final int remaining;
  final bool owner;

  /// The newest payload version the home reads, in the flags' high four
  /// bits (spec "Mailbox", The answer).
  final int newest;
  final Outcome? outcome;
  final List<PhoneRecord>? list;
  const RoundSummary(
    this.items,
    this.remaining, {
    this.owner = false,
    this.newest = formatVersion,
    this.outcome,
    this.list,
  });
  bool get nothingNew => items == 0;

  Uint8List encode() => Uint8List.fromList([
    ...be(items, itemsLen),
    ...be(remaining, remainingLen),
    newest << 4 | (owner ? 1 : 0),
    ...?switch (outcome) {
      null => null,
      final o => [o.index],
    },
    for (final r in list ?? const <PhoneRecord>[]) ...r.encode(),
  ]);

  /// The plaintext `b` holds whole.
  static RoundSummary decode(List<int> b) {
    const fixed = itemsLen + remainingLen + flagsLen;
    if (b.length < fixed) throw const BadPayload('not a summary');
    final rest = b.sublist(fixed);
    final code = rest.firstOrNull;
    if (code != null && code >= Outcome.values.length) {
      throw const BadPayload('no such outcome');
    }
    final outcome = code == null ? null : Outcome.values[code];
    if (rest.length > 1 && outcome != Outcome.ok) {
      throw const BadPayload('records after a refusal');
    }
    final List<PhoneRecord>? list;
    try {
      list = rest.length > 1 ? decodeRecords(rest.sublist(1)) : null;
    } on Malformed {
      throw const BadPayload('not a list');
    }
    return RoundSummary(
      fromBe(b.sublist(0, itemsLen)),
      fromBe(b.sublist(itemsLen, itemsLen + remainingLen)),
      owner: b[itemsLen + remainingLen] & 1 != 0,
      newest: b[itemsLen + remainingLen] >> 4,
      outcome: outcome,
      list: list,
    );
  }
}

/// The summary of the fetch of [round], as the home sends it for the
/// fetch's [recipient] tag, sealed under the reply key at place 0.
Uint8List sealSummary(
  Sodium s,
  SecureKey key,
  int recipient,
  int round,
  RoundSummary summary,
) => Uint8List.fromList([
  formatByte(Form.summary),
  recipient,
  ..._underReplyKey(
    s,
    key,
    (k) => s.crypto.secretBox.easy(
      message: summary.encode(),
      nonce: replyNonce(round, 0),
      key: k,
    ),
  ),
]);

/// The summary of the fetch of [round], or null if it is not one of its.
RoundSummary? openSummary(
  Sodium s,
  SecureKey key,
  int round,
  List<int> payload,
) {
  if (formOf(payload) != Form.summary ||
      payload.length < summaryLen ||
      payload.length > maxPayload) {
    return null;
  }
  try {
    final p = _underReplyKey(
      s,
      key,
      (k) => s.crypto.secretBox.openEasy(
        cipherText: Uint8List.fromList(payload.sublist(headLen)),
        nonce: replyNonce(round, 0),
        key: k,
      ),
    );
    return RoundSummary.decode(p);
  } on SodiumException {
    return null;
  } on BadPayload {
    return null;
  }
}

/// A kept boxed payload or introduction at [place] in [round], as the home
/// sends it for the fetch's [recipient] tag: its form says the item's, and
/// the item without its format byte and tag goes under the reply key's
/// stream alone, as it proves itself.
Uint8List sealItem(
  Sodium s,
  SecureKey key,
  int recipient,
  int round,
  int place,
  List<int> item,
) {
  final form = switch (formOf(item)) {
    Form.boxed => Form.boxedItem,
    Form.introduction => Form.introductionItem,
    _ => throw const BadPayload('not an item a reply carries'),
  };
  if (place < 1 || place > maxReplies || item.length > maxAppPayload) {
    throw const BadPayload('not an item a reply carries');
  }
  return Uint8List.fromList([
    formatByte(form),
    recipient,
    place,
    ..._underReplyKey(
      s,
      key,
      (k) => stream(s, k, replyNonce(round, place), item.sublist(headLen)),
    ),
  ]);
}

/// The item a reply of [round] carries, its format byte from the reply's
/// form and the reply's recipient tag in place of its own; null if it is
/// not an item reply. Under another round the bytes are noise: only the
/// item opening says it was this round's.
Uint8List? openItem(Sodium s, SecureKey key, int round, List<int> payload) {
  final form = switch (formOf(payload)) {
    Form.boxedItem => Form.boxed,
    Form.introductionItem => Form.introduction,
    _ => null,
  };
  if (form == null ||
      payload.length < headLen + placeLen ||
      payload.length > maxPayload) {
    return null;
  }
  final place = payload[headLen];
  if (place < 1 || place > maxReplies) return null;
  return Uint8List.fromList([
    formatByte(form),
    payload[1],
    ..._underReplyKey(
      s,
      key,
      (k) => stream(
        s,
        k,
        replyNonce(round, place),
        payload.sublist(headLen + placeLen),
      ),
    ),
  ]);
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

/// The most bytes of a reaction's emoji: the longest in Unicode 17.0's
/// emoji list, a kiss of two people each with a skin tone, ten code
/// points joined (`bark-app-layer` `plain.rs`).
const int maxReaction = 35;

/// Whether `emoji` may be a reaction: one character as a person sees it,
/// of at most [maxReaction] bytes. That it is an emoji and not a letter
/// is the picker's to offer.
bool isReaction(String emoji) =>
    emoji.characters.length == 1 && utf8.encode(emoji).length <= maxReaction;

/// A reaction, one emoji, to the text whose first part was sent at
/// [sent]. It runs to the end, so it comes last.
class ReactionRecord extends Record {
  final int sent;
  final String emoji;
  const ReactionRecord(this.sent, this.emoji);
}

class CardRecord extends Record {
  /// The card's bytes.
  final Uint8List card;
  CardRecord(List<int> card) : card = Uint8List.fromList(card);
}

/// A home the sender, its owner, gives the receiver: the home node's
/// address and the mailbox key made for them there (spec "Phone link",
/// The payload).
class HomeRecord extends Record {
  final Address home;
  final Uint8List key;
  HomeRecord(this.home, List<int> key) : key = Uint8List.fromList(key);
  static const len = Address.len + keyLen;
}

const int _typeUtf8 = 1;
const int _type1251 = 2;
const int _typeReaction = 3;
const int _typeCard = 4;
const int _typeUtf8Part = 5;
const int _type1251Part = 6;
const int _typeHome = 7;

/// The most parts a text is split into: a part's number and the count
/// share one byte, four bits each.
const int maxParts = 16;

/// A plaintext: the header, then the records, a text or a reaction last.
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
      case ReactionRecord(:final sent, :final emoji):
        if (i != records.length - 1) {
          throw const BadPayload('reaction not last');
        }
        if (!isReaction(emoji)) throw const BadPayload('not one emoji');
        b.addAll([_typeReaction, ...be(sent, 4), ...utf8.encode(emoji)]);
      case CardRecord(:final card):
        b.addAll([_typeCard, card.length, ...card]);
      case HomeRecord(:final home, :final key):
        b.addAll([_typeHome, ...home.encode(), ...key]);
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
      case _typeReaction when at + 4 <= p.length:
        final String emoji;
        try {
          emoji = utf8.decode(p.sublist(at + 4));
        } on FormatException {
          throw const BadPayload('reaction not UTF-8');
        }
        if (!isReaction(emoji)) throw const BadPayload('not one emoji');
        records.add(ReactionRecord(fromBe(p.sublist(at, at + 4)), emoji));
        at = p.length;
      case _typeCard when at < p.length && at + 1 + p[at] <= p.length:
        records.add(CardRecord(p.sublist(at + 1, at + 1 + p[at])));
        at += 1 + p[at];
      case _typeHome when at + HomeRecord.len <= p.length:
        final a = p.sublist(at, at + Address.len);
        records.add(
          HomeRecord(
            Address(Position.decode(a), a.sublist(Position.wireLen)),
            p.sublist(at + Address.len, at + HomeRecord.len),
          ),
        );
        at += HomeRecord.len;
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
