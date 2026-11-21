// Moving to a new phone: the person's identity file, saved here and
// restored there (`identity_file.dart`).

part of '../app.dart';

mixin _Moving on _AppCore {
  PasswordKey get passwordKey;

  /// The person's identity file under `password`, their messages in it if
  /// `messages`.
  Future<Uint8List> identityFile(String password, {required bool messages}) {
    final h = me.home;
    final f = IdentityFile(
      seed: me.keys.seed,
      name: me.name,
      home: h == null ? null : (address: h, key: me.mailboxKey.extractBytes()),
      nodeAddress: me.nodeAddress,
      nodeAt: me.nodeAt,
      cardTime: me.cardTime,
      cardSentTime: me.cardSentTime,
      lastSent: me.lastSent,
      backedUp: me.backedUp,
      remaining: me.remaining,
      rounds: [...me.rounds],
      contacts: [...contacts.list],
      archived: {...contacts.archived},
      nodes: [
        for (final r in nodes.records)
          if (r.paired || r.ringAfter != 0)
            (name: r.name, paired: r.paired, ringAfter: r.ringAfter),
      ],
      messages: messages
          ? (
              outbox: [...session.outbox],
              received: [...session.received],
              reactionsUsed: {...session.reactionsUsed},
            )
          : null,
    );
    return f.seal(sodium, passwordKey, password);
  }

  /// Replaces the person, their contacts, their nodes and the messages by
  /// those of `file`, once it opens under `password`; otherwise throws a
  /// [FileRefused] and changes nothing. The link ends first: it was made
  /// for the identity replaced. The nodes the old phone was paired with
  /// wait to be paired again, as a pairing stays in the phone it was made
  /// on; the home needs none, as it holds the mailbox key.
  Future<void> restoreFile(Uint8List file, String password) async {
    final f = await IdentityFile.open(sodium, passwordKey, password, file);
    await session.disconnect();
    final m = f.messages;
    // Each applies at once and saves after, so that nothing runs between
    // them on a state of two identities.
    await Future.wait([
      me.moveIn(f),
      contacts.moveIn(f.contacts, f.archived),
      session.moveIn(
        f.nodes,
        outbox: m?.outbox ?? const [],
        received: m?.received ?? const [],
        reactionsUsed: m?.reactionsUsed ?? const {},
      ),
    ]);
    session.note(
      'Identity restored from a file, ${f.contacts.length} contacts, '
      '${m == null ? 'no' : m.received.length + m.outbox.length} messages',
    );
    notifyListeners();
  }

  /// Replaces the person by the one of `seed`, from their 12 words. The
  /// link ends first, as for a file: the node gives it the mail of the
  /// identity replaced.
  Future<void> restoreWords(Uint8List seed) async {
    await session.disconnect();
    await me.restore(seed);
    session.note('Identity restored from words');
    notifyListeners();
  }
}
