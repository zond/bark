// Texts, reactions, cards and fetches, sealed for the network and
// opened from it (spec "Phone link", "Mailbox"): what the person sends
// and receives, on top of the session's queue and inbox.

import 'dart:typed_data';

import 'package:sodium/sodium.dart';

import 'app_layer.dart';
import 'card.dart';
import 'contacts.dart';
import 'link.dart';
import 'me.dart';
import 'session.dart';

class Messenger {
  final Sodium sodium;
  final Me me;
  final Contacts contacts;
  final Session session;
  final DateTime Function() now;

  Messenger(this.sodium, this.me, this.contacts, this.session, this.now) {
    session.reader = receive;
    session.homeKey = (n) =>
        isHome(n.id) ? me.keys.mailbox.extractBytes() : null;
    session.recipient = (n) => recipientTag(sodium, me.identity, n.id);
    session.name = () => me.name;
  }

  /// Whether the node of `id` is the person's home: a home is known by its
  /// node ID, as its position may be corrected.
  bool isHome(List<int> id) {
    final h = me.home;
    return h != null && hex(h.id) == hex(id);
  }

  /// `crypto_box_beforenm` of each contact, made once; keyed by both
  /// identities, so a restored identity makes its own.
  final Map<String, PrecalculatedBox> _pairs = {};

  PrecalculatedBox _pair(List<int> them) => _pairs.putIfAbsent(
    '${me.key}/${hex(them)}',
    () => sodium.crypto.box.precalculate(
      publicKey: Uint8List.fromList(them),
      secretKey: me.keys.pair.secretKey,
    ),
  );

  /// Seals `records` to `c` at `dst`: boxed once `c` holds the person's
  /// card, else a first message with it. Being verified cannot choose:
  /// only a box verifies, so two people waiting for it would never send
  /// one. The header names the node the person writes from, the one
  /// connected or the last, until `c` has answered a message that named
  /// it. Returns the payload and its `sent`.
  Future<(Uint8List, int)> _seal(
    Contact c,
    Address dst,
    List<Record> records, {
    int? sent,
  }) async {
    final card = me.card;
    final here = me.nodeAddress;
    if (!c.hasMine && card.home == null && here == null) {
      throw const BadPayload('no address to answer to yet');
    }
    sent ??= await me.nextSent();
    final withAddress = _withAddress(c);
    final header = Header(
      greater(me.identity, c.card.identity),
      sent,
      withAddress ? here : null,
    );
    if (withAddress) {
      await contacts.put(c.copyWith(sentMine: here.toString()));
    }
    final to = recipientTag(sodium, c.card.identity, dst.id);
    if (!c.hasMine) {
      final plain = writePlain(header, [CardRecord(card.encode()), ...records]);
      return (sealIntro(sodium, c.card.identity, to, plain), sent);
    }
    return (
      sealBox(
        _pair(c.card.identity),
        to,
        sodium.randombytes.buf(nonceLen),
        writePlain(header, records),
      ),
      sent,
    );
  }

  /// Whether the header names the node the person writes from: until `c`
  /// has answered a message that named it.
  bool _withAddress(Contact c) {
    final here = me.nodeAddress;
    return here != null && c.knowsMine != here.toString();
  }

  /// The plaintext's room for records in a message to `c` now: a first
  /// message also carries the person's card.
  int _recordRoom(Contact c) =>
      maxAppPayload -
      (c.hasMine ? boxOverhead : introOverhead + 2 + me.card.encode().length) -
      5 -
      (_withAddress(c) ? Address.len : 0);

  /// `text` as the parts a message to `c` holds now, one if it fits.
  List<String> partsOf(Contact c, String text) =>
      splitText(text, _recordRoom(c) - 1);

  /// Sends a text to `c`, to the address `toward` names, by default the
  /// one the person need not choose; split into parts, queued in order,
  /// if one message does not hold it.
  Future<void> sendText(Contact c, String text, {Toward? toward}) async {
    final t = toward ?? c.unasked(now());
    if (t == null) throw BadPayload('${c.name} has no address yet');
    final parts = partsOf(c, text);
    final n = parts.length;
    if (n > maxParts) throw BadPayload('too long: $n parts, $maxParts at most');
    final first = await me.nextSents(n);
    final dst = c.addressOf(t);
    for (final (i, part) in parts.indexed) {
      final (p, sent) = await _seal(c, dst, [
        TextRecord(part, i + 1, n),
      ], sent: first + i);
      await session.queue(
        Outgoing(
          dst,
          p,
          at: now(),
          kind: OutKind.text,
          to: c.key,
          sent: sent,
          body: part,
          toward: t,
          part: i + 1,
          of: n,
        ),
      );
    }
  }

  /// Sends the person's reaction, one emoji, to the text of `r`, naming
  /// it by its first part's `sent`; one replaces the one before, and goes
  /// again even if the same, as the person asked. Throws [BadPayload] if
  /// `emoji` is not one character.
  Future<void> react(Received r, String emoji) async {
    final c = contacts.byKey(r.from);
    final t = c?.unasked(now());
    final first = r.first;
    if (c == null || t == null || first == null) return;
    final dst = c.addressOf(t);
    final (p, sent) = await _seal(c, dst, [ReactionRecord(first, emoji)]);
    for (final x in session.partsOf(r)) {
      x?.reaction = emoji;
    }
    session.usedReaction(emoji);
    await session.queue(
      Outgoing(
        dst,
        p,
        at: now(),
        kind: OutKind.reaction,
        to: c.key,
        sent: sent,
        body: emoji,
        toward: t,
      ),
    );
  }

  /// Sends the person's card to `c`.
  Future<void> sendCard(Contact c) async {
    final t = c.unasked(now());
    if (t == null) return;
    final dst = c.addressOf(t);
    final (p, sent) = await _seal(c, dst, [CardRecord(me.card.encode())]);
    await session.queue(
      Outgoing(
        dst,
        p,
        at: now(),
        kind: OutKind.card,
        to: c.key,
        sent: sent,
        toward: t,
      ),
    );
  }

  /// Sends the person's card to every contact. Only the person's word
  /// sends it, as a card to every contact on each change of the home's
  /// position flooded the mesh (roadmap D126).
  Future<void> sendCardToAll() async {
    final t = me.cardTime;
    for (final c in [...contacts.list]) {
      await sendCard(c);
    }
    await me.setCardSent(t);
  }

  /// Asks the home node for the newest batch of the items it has not
  /// sent, to be sent to `here`: the fetch carries the person's tag at the
  /// home, and inside, for the replies, their tag at `here`.
  Future<void> fetch(Address here) async {
    final home = me.home;
    if (home == null) return;
    final sent = await me.nextSent();
    final f = Fetch(sent, here, recipientTag(sodium, me.identity, here.id));
    final p = f.seal(
      sodium,
      me.keys.mailbox,
      recipientTag(sodium, me.identity, home.id),
      sodium.randombytes.buf(nonceLen),
    );
    await me.fetched(sent);
    await session.queue(
      Outgoing(home, p, at: now(), kind: OutKind.fetch, sent: sent),
    );
  }

  /// Reads one message from the node's inbox and keeps what it says.
  Future<void> receive(InboxMessage m) async {
    if (!await _open(m, m.payload, mailbox: false)) {
      if (session.received.any((r) => r.id == m.msgId && r.from == null)) {
        return;
      }
      await session.addReceived(Received(m.msgId, m.payload, now()));
    }
  }

  /// Opens a payload; false if it opens for nobody here.
  Future<bool> _open(
    InboxMessage m,
    List<int> p, {
    required bool mailbox,
  }) async {
    switch (formOf(p)) {
      case Form.reply:
        return _reply(m, p);
      case Form.boxed:
        for (final c in [...contacts.list, null]) {
          final them = c?.card.identity ?? me.identity;
          final Uint8List plain;
          try {
            plain = openBox(_pair(them), p);
          } on BadPayload {
            continue;
          }
          return _boxed(m, them, c, plain, mailbox);
        }
        return false;
      case Form.introduction:
        try {
          return await _intro(m, openIntro(sodium, me.keys, p), mailbox);
        } on BadPayload {
          return false;
        }
      case Form.fetch || null:
        return false;
    }
  }

  Future<bool> _reply(InboxMessage m, List<int> p) async {
    for (final round in me.rounds) {
      final o = openReply(sodium, me.keys.mailbox, round, p);
      if (o == null) continue;
      await session.fetchAnswered(
        round,
        nothingNew: o.nothingNew,
        remaining: round == me.rounds.first ? o.remaining : 0,
      );
      await me.gotReply(round, o);
      final item = o.item;
      // An empty item is one the home could not send: only its number came.
      if (item == null || item.isEmpty) return true;
      // An item that opens for nobody here is still one the mailbox has
      // delivered: it is kept as received.
      if (!await _open(m, item, mailbox: true)) {
        await session.addReceived(
          Received(m.msgId, item, now(), mailbox: true),
        );
      }
      return true;
    }
    return false;
  }

  Future<bool> _boxed(
    InboxMessage m,
    Uint8List them,
    Contact? c,
    Uint8List plain,
    bool mailbox,
  ) async {
    final Header h;
    final List<Record> rs;
    try {
      (h, rs) = readPlain(plain);
    } on BadPayload {
      return false;
    }
    // A message of the person's own, sent back to them.
    if (h.greater != greater(them, me.identity)) return false;
    if (c != null) {
      await contacts.put(
        c
            .copyWith(verified: true, knowsMine: c.sentMine)
            .heard(h.sent, h.address, now()),
      );
    }
    await _records(m, hex(them), h, rs, mailbox);
    return true;
  }

  Future<bool> _intro(InboxMessage m, Uint8List plain, bool mailbox) async {
    final Header h;
    final List<Record> rs;
    final PersonCard card;
    try {
      (h, rs) = readPlain(plain);
      card = PersonCard.decode((rs.first as CardRecord).card);
    } catch (_) {
      return false;
    }
    if (h.greater != greater(card.identity, me.identity)) return false;
    final known = contacts.byKey(card.key);
    if (known != null && known.verified) {
      // A first message of theirs that a box overtook on the way, or
      // anyone's who holds both cards: its text is kept, marked, but it
      // changes nothing a box has proven, so a forger can only add
      // marked texts.
      await _records(
        m,
        card.key,
        h,
        rs.whereType<TextRecord>(),
        mailbox,
        intro: true,
      );
      return true;
    }
    await contacts.put(
      (known ?? Contact(card, '', cameByMessage: true))
          .withCard(card)
          .heard(h.sent, h.address, now()),
    );
    await _records(m, card.key, h, rs.skip(1), mailbox, intro: true);
    return true;
  }

  Future<void> _records(
    InboxMessage m,
    String from,
    Header h,
    Iterable<Record> rs,
    bool mailbox, {
    bool intro = false,
  }) async {
    for (final r in rs) {
      switch (r) {
        case TextRecord(:final text, :final part, :final of):
          final dup = session.received.any(
            (x) => x.from == from && x.sent == h.sent,
          );
          if (!dup) {
            // A new text brings an archived conversation back before it
            // is told, so the person finds it where the notification
            // leads.
            await contacts.unarchive(from);
            await session.addReceived(
              Received(
                m.msgId,
                m.payload,
                now(),
                from: from,
                sent: h.sent,
                body: text,
                mailbox: mailbox,
                part: part,
                of: of,
                intro: intro,
              ),
            );
          }
        case ReactionRecord(:final sent, :final emoji):
          final parts = [
            for (final o in session.outbox)
              if (o.to == from && o.kind == OutKind.text && o.first == sent) o,
          ];
          if (parts.isEmpty || h.sent <= parts.first.reactionSent) break;
          for (final o in parts) {
            o.reaction = emoji;
            o.reactionSent = h.sent;
          }
          await session.save();
        case CardRecord(:final card):
          await _card(from, card);
      }
    }
  }

  /// A card in a message: the sender's own replaces their card unless it
  /// is older; another's is a new contact, unless it is known.
  Future<void> _card(String from, Uint8List bytes) async {
    final PersonCard card;
    try {
      card = PersonCard.decode(bytes);
    } on BadCard {
      return;
    }
    final c = contacts.byKey(card.key);
    if (c == null) {
      if (card.key != me.key) {
        await contacts.put(Contact(card, '').withCard(card));
      }
    } else if (card.time >= c.card.time) {
      // Of the same time, it may name the node they use now.
      await contacts.put(c.withCard(card));
    }
  }
}
