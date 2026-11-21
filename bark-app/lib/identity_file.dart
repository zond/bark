// The identity file: what a person needs to go on from a new phone, sealed
// under a password they choose. The 12 words bring back the identity
// alone; the file brings back the name, the home, the contacts, the nodes
// and, if asked, the messages too.
//
// The file is a header, then the contents as JSON, encrypted and
// authenticated with XChaCha20-Poly1305 under a key Argon2id makes of the
// password and the header's salt; the header is the cipher's additional
// data, so a changed byte anywhere fails the open.

import 'dart:convert';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:flutter/foundation.dart' show listEquals;
import 'package:sodium/sodium_sumo.dart';

import 'app_layer.dart';
import 'contacts.dart';
import 'link.dart';
import 'nodes.dart';
import 'session.dart';

/// The layout's version, the header's byte after [magic]; a stored layout,
/// so it counts up at every change (AGENTS.md, "Versions before the first
/// deployment").
const int identityFileVersion = 1;

/// What a file starts with, so that another file is told apart from a
/// damaged one.
final Uint8List magic = ascii.encode('bark-identity');

/// Argon2id's salt, [Pwhash.saltBytes]: 16 bytes, the 128 bits RFC 9106
/// §4 recommends.
const int saltLen = 16;

/// XChaCha20-Poly1305's nonce, random: 24 bytes are long enough that two
/// random ones do not meet.
const int fileNonceLen = 24;

/// The header: [magic], the version, the salt and the nonce.
final int headerLen = magic.length + 1 + saltLen + fileNonceLen;

/// Argon2id's cost, RFC 9106 §4's second recommended option, for where
/// its first one's 2 GiB is more memory than there is: 3 passes over
/// 64 MiB. libsodium runs it in one lane, not that option's four, so it
/// takes longer on the phone; the memory, which is what each guess at
/// the password costs, is the same.
const int hashPasses = 3;
const int hashMemory = 64 << 20;

/// The key of `password` and `salt`: [argon2id] on the phone.
typedef PasswordKey = Future<Uint8List> Function(
  String password,
  Uint8List salt,
);

/// libsodium's Argon2id of [hashPasses] over [hashMemory], in an isolate
/// of its own, as it takes long enough to stall the screen.
Future<Uint8List> argon2id(String password, Uint8List salt) =>
    Isolate.run(() async {
      final s = await SodiumSumoInit.init();
      final k = s.crypto.pwhash.callStr(
        outLen: keyLen,
        password: password,
        salt: salt,
        opsLimit: hashPasses,
        memLimit: hashMemory,
        alg: CryptoPwhashAlgorithm.argon2id13,
      );
      try {
        return k.extractBytes();
      } finally {
        k.dispose();
      }
    });

/// Why a file was not read; nothing changed.
sealed class FileRefused implements Exception {
  const FileRefused();
}

/// The file does not start as an identity file does.
class NotAnIdentityFile extends FileRefused {
  const NotAnIdentityFile();
  @override
  String toString() => 'This is not a bark identity file.';
}

/// The file is of a layout this app does not read.
class OtherLayout extends FileRefused {
  const OtherLayout();
  @override
  String toString() => 'This file is from another version of the app.';
}

/// The file does not open under the password: the cipher cannot tell a
/// wrong password from a changed byte.
class WrongPassword extends FileRefused {
  const WrongPassword();
  @override
  String toString() => 'Wrong password, or the file is damaged.';
}

/// The person's home: its address and the mailbox key the person gave
/// it, kept as it is, as a home's key may be one the person was given
/// rather than one made of their identity.
typedef FileHome = ({Address address, Uint8List key});

/// The messages: the queue and what was received, and the reactions the
/// person used most.
typedef FileMessages = ({
  List<Outgoing> outbox,
  List<Received> received,
  Map<String, int> reactionsUsed,
});

/// What the file holds.
class IdentityFile {
  final Uint8List seed;
  final String name;
  final FileHome? home;

  /// The node the old phone was linked to last, and when, as `sent`: the
  /// card names it.
  final Address? nodeAddress;
  final int nodeAt;

  /// The card's time and that of the one last sent to every contact.
  final int cardTime;
  final int cardSentTime;

  /// The last `sent` given, so that the new phone's come after it: a
  /// contact takes only a newer `sent` as news, and the home answers only
  /// a fetch later than its last.
  final int lastSent;
  final bool backedUp;

  /// The mailbox: what the home last said it keeps, and the latest
  /// fetches' rounds, whose replies may still come.
  final int remaining;
  final List<int> rounds;

  final List<Contact> contacts;
  final Set<String> archived;
  final List<MovedNode> nodes;

  /// Null when the person left the messages out.
  final FileMessages? messages;

  const IdentityFile({
    required this.seed,
    required this.name,
    required this.home,
    required this.nodeAddress,
    required this.nodeAt,
    required this.cardTime,
    required this.cardSentTime,
    required this.lastSent,
    required this.backedUp,
    required this.remaining,
    required this.rounds,
    required this.contacts,
    required this.archived,
    required this.nodes,
    required this.messages,
  });

  /// The contents. The contacts and the messages are in the forms of their
  /// own files, each with that file's version: a change to either form is
  /// then a file this app refuses, not one it misreads.
  Map<String, dynamic> toJson() => {
    'seed': hex(seed),
    'name': name,
    'home': switch (home) {
      null => null,
      final h => {'address': h.address.toString(), 'key': hex(h.key)},
    },
    'nodeAddress': nodeAddress?.toString(),
    'nodeAt': nodeAt,
    'cardTime': cardTime,
    'cardSentTime': cardSentTime,
    'lastSent': lastSent,
    'backedUp': backedUp,
    'remaining': remaining,
    'rounds': rounds,
    'contacts': {
      'version': Contacts.version,
      'contacts': [for (final c in contacts) c.toJson()],
      'archived': [...archived],
    },
    'nodes': [
      for (final n in nodes)
        {
          'name': n.name,
          'paired': n.paired,
          'lost': n.lost,
          'ringAfter': n.ringAfter,
        },
    ],
    'messages': switch (messages) {
      null => null,
      final m => {
        'version': Session.version,
        'outbox': [for (final o in m.outbox) o.toJson()],
        'received': [for (final r in m.received) r.toJson()],
        'reactionsUsed': m.reactionsUsed,
      },
    },
  };

  /// Reads the contents; throws [OtherLayout] for a form this app does not
  /// write, and a [TypeError] or [FormatException] for a field that does
  /// not read.
  static IdentityFile fromJson(Map<String, dynamic> j) {
    final c = j['contacts'] as Map<String, dynamic>;
    if (c['version'] != Contacts.version) throw const OtherLayout();
    final m = j['messages'] as Map<String, dynamic>?;
    if (m != null && m['version'] != Session.version) {
      throw const OtherLayout();
    }
    final h = j['home'] as Map<String, dynamic>?;
    return IdentityFile(
      seed: _bytes(j['seed']),
      name: j['name'] as String,
      home: h == null
          ? null
          : (address: _address(h['address'])!, key: _bytes(h['key'])),
      nodeAddress: _address(j['nodeAddress']),
      nodeAt: j['nodeAt'] as int,
      cardTime: j['cardTime'] as int,
      cardSentTime: j['cardSentTime'] as int,
      lastSent: j['lastSent'] as int,
      backedUp: j['backedUp'] as bool,
      remaining: j['remaining'] as int,
      rounds: (j['rounds'] as List).cast<int>().toList(),
      contacts: [
        for (final e in c['contacts'] as List)
          Contact.fromJson(e as Map<String, dynamic>),
      ],
      archived: {for (final k in c['archived'] as List) k as String},
      nodes: [
        for (final e in j['nodes'] as List)
          (
            name: (e as Map<String, dynamic>)['name'] as String,
            paired: e['paired'] as bool,
            lost: e['lost'] as bool,
            ringAfter: e['ringAfter'] as int,
          ),
      ],
      messages: m == null
          ? null
          : (
              outbox: [
                for (final e in m['outbox'] as List)
                  Outgoing.fromJson(e as Map<String, dynamic>),
              ],
              received: [
                for (final e in m['received'] as List)
                  Received.fromJson(e as Map<String, dynamic>),
              ],
              reactionsUsed: {
                for (final e
                    in (m['reactionsUsed'] as Map<String, dynamic>).entries)
                  e.key: e.value as int,
              },
            ),
    );
  }

  static Uint8List _bytes(Object? s) =>
      parseHex(s as String) ?? (throw FormatException('hex $s'));

  static Address? _address(Object? s) => switch (s) {
    null => null,
    final a =>
      Address.parse(a as String) ?? (throw FormatException('address $a')),
  };

  /// The file of these contents under `password`, with a fresh salt and
  /// nonce.
  Future<Uint8List> seal(Sodium s, PasswordKey key, String password) async {
    final salt = s.randombytes.buf(saltLen);
    final nonce = s.randombytes.buf(fileNonceLen);
    final header = Uint8List.fromList([
      ...magic,
      identityFileVersion,
      ...salt,
      ...nonce,
    ]);
    final k = SecureKey.fromList(s, await key(password, salt));
    try {
      final sealed = s.crypto.aeadXChaCha20Poly1305IETF.encrypt(
        message: utf8.encode(jsonEncode(toJson())),
        additionalData: header,
        nonce: nonce,
        key: k,
      );
      return Uint8List.fromList([...header, ...sealed]);
    } finally {
      k.dispose();
    }
  }

  /// The contents of `file` under `password`; throws a [FileRefused].
  static Future<IdentityFile> open(
    Sodium s,
    PasswordKey key,
    String password,
    Uint8List file,
  ) async {
    if (file.length <= magic.length ||
        !listEquals(file.sublist(0, magic.length), magic)) {
      throw const NotAnIdentityFile();
    }
    if (file[magic.length] != identityFileVersion) throw const OtherLayout();
    if (file.length < headerLen + s.crypto.aeadXChaCha20Poly1305IETF.aBytes) {
      throw const WrongPassword();
    }
    final header = file.sublist(0, headerLen);
    final salt = header.sublist(magic.length + 1, magic.length + 1 + saltLen);
    final nonce = header.sublist(headerLen - fileNonceLen);
    final k = SecureKey.fromList(s, await key(password, salt));
    final Uint8List plain;
    try {
      plain = s.crypto.aeadXChaCha20Poly1305IETF.decrypt(
        cipherText: file.sublist(headerLen),
        additionalData: header,
        nonce: nonce,
        key: k,
      );
    } on SodiumException {
      throw const WrongPassword();
    } finally {
      k.dispose();
    }
    try {
      final f = fromJson(
        jsonDecode(utf8.decode(plain)) as Map<String, dynamic>,
      );
      // This app uses only the mailbox key its identity makes; a file
      // whose home has another was written by one that keeps given keys.
      final h = f.home;
      final keys = Keys.fromSeed(s, f.seed);
      if (h != null && hex(keys.mailbox.extractBytes()) != hex(h.key)) {
        throw const OtherLayout();
      }
      return f;
    } on FileRefused {
      rethrow;
    } catch (_) {
      // It opened, so an app wrote it: one whose contents this app does
      // not read.
      throw const OtherLayout();
    }
  }
}
