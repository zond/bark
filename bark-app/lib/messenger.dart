// Texts, acknowledgements, cards and fetches, sealed for the network and
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

  /// Seals `records` to `c`: boxed once `c` is verified, else a first
  /// message with the person's card. The header names the node the
  /// person writes from, the one connected or the last, until `c` has
  /// answered a message that named it. Returns the payload and its `sent`.
  Future<(Uint8List, int)> _seal(Contact c, List<Record> records) async {
    final sent = await me.nextSent();
    final card = me.card;
    final here = me.nodeAddress;
    if (!c.verified && card.home == null && here == null) {
      throw const BadPayload('no address to answer to yet');
    }
    final withAddress = here != null && c.knowsMine != here.toString();
    final header = Header(
      greater(me.identity, c.card.identity),
      sent,
      withAddress ? here : null,
    );
    if (withAddress) {
      await contacts.put(c.copyWith(sentMine: here.toString()));
    }
    if (!c.verified) {
      final plain = writePlain(header, [CardRecord(card.encode()), ...records]);
      return (sealIntro(sodium, c.card.identity, plain), sent);
    }
    return (
      sealBox(
        _pair(c.card.identity),
        sodium.randombytes.buf(nonceLen),
        writePlain(header, records),
      ),
      sent,
    );
  }

  /// Sends a text to `c`, to the address `toward` names, by default the
  /// one the person need not choose.
  Future<void> sendText(Contact c, String text, {Toward? toward}) async {
    final t = toward ?? c.unasked(now());
    if (t == null) throw BadPayload('${c.name} has no address yet');
    final (p, sent) = await _seal(c, [TextRecord(text)]);
    await session.queue(
      Outgoing(
        c.addressOf(t),
        p,
        at: now(),
        kind: OutKind.text,
        to: c.key,
        sent: sent,
        body: text,
        toward: t,
      ),
    );
  }

  /// Acknowledges a received text: a record naming it, and no text.
  Future<void> acknowledge(Received r) async {
    final c = contacts.byKey(r.from);
    final named = r.sent;
    final t = c?.unasked(now());
    if (c == null || t == null || named == null || r.acked) return;
    final (p, sent) = await _seal(c, [AckRecord(named)]);
    r.acked = true;
    await session.queue(
      Outgoing(
        c.addressOf(t),
        p,
        at: now(),
        kind: OutKind.ack,
        to: c.key,
        sent: sent,
        toward: t,
      ),
    );
  }

  /// Sends the person's card to `c`.
  Future<void> sendCard(Contact c) async {
    final t = c.unasked(now());
    if (t == null) return;
    final (p, sent) = await _seal(c, [CardRecord(me.card.encode())]);
    await session.queue(
      Outgoing(
        c.addressOf(t),
        p,
        at: now(),
        kind: OutKind.card,
        to: c.key,
        sent: sent,
        toward: t,
      ),
    );
  }

  Future<void> sendCardToAll() async {
    for (final c in [...contacts.list]) {
      await sendCard(c);
    }
  }

  /// Asks the home node for every item after this phone's `since`, to be
  /// sent to `here`.
  Future<void> fetch(Address here) async {
    final home = me.home;
    if (home == null) return;
    final sent = await me.nextSent();
    final f = Fetch(sent, here, me.since);
    final p = f.seal(sodium, me.keys.mailbox, sodium.randombytes.buf(nonceLen));
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
      await session.fetchAnswered(round, nothingNew: o.nothingNew);
      final item = o.item;
      if (item == null) return true;
      await me.gotItem(o.number, first: o.number - o.place + 1);
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
    // Only the holder of a verified identity can box; a sealed box
    // naming it may be anyone's.
    if (known != null && known.verified) return true;
    await contacts.put(
      (known?.copyWith(card: card) ??
              Contact(card, card.name.isEmpty ? 'Unnamed' : card.name))
          .heard(h.sent, h.address, now()),
    );
    await _records(m, card.key, h, rs.sublist(1), mailbox);
    return true;
  }

  Future<void> _records(
    InboxMessage m,
    String from,
    Header h,
    List<Record> rs,
    bool mailbox,
  ) async {
    for (final r in rs) {
      switch (r) {
        case TextRecord(:final text):
          final dup = session.received.any(
            (x) => x.from == from && x.sent == h.sent,
          );
          if (!dup) {
            await session.addReceived(
              Received(
                m.msgId,
                m.payload,
                now(),
                from: from,
                sent: h.sent,
                body: text,
                mailbox: mailbox,
              ),
            );
          }
        case AckRecord(:final sent):
          for (final o in session.outbox) {
            if (o.to == from && o.sent == sent && o.kind == OutKind.text) {
              o.acked = true;
            }
          }
          await session.save();
        case CardRecord(:final card):
          await _card(from, card);
      }
    }
  }

  /// A card in a message: the sender's own replaces their address if it
  /// is later; another's is a new contact, unless it is known.
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
        await contacts.put(
          Contact(card, card.name.isEmpty ? 'Unnamed' : card.name),
        );
      }
    } else if (card.time > c.card.time) {
      await contacts.put(c.copyWith(card: card));
    }
  }
}
