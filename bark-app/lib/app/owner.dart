// The home's owner: the people whose home it is, added, removed and the
// mark handed on by commands on a fetch from anywhere, and at the node by
// the code on its screen (spec "Phone link", The node's owner; "Mailbox",
// The owner's commands).

part of '../app.dart';

mixin _Owner on _AppCore {
  /// The person's record at their home, as Phones lists it while the
  /// phone is paired there; null elsewhere.
  PhoneRecord? get _ownRecord {
    if (!atHome || session.guest) return null;
    return session.phones?.where((r) => r.mine).firstOrNull;
  }

  /// The home holds the person's key as its owner's: as Phones says it at
  /// the home, else as its last summary said.
  bool get isOwner => _ownRecord?.owner ?? me.owner;

  /// Keeps what Phones says at the home, so that it holds away from it.
  @override
  void _ownerSeen() {
    final r = _ownRecord;
    if (r != null) unawaited(me.setOwner(r.owner));
  }

  /// The home's records as the owner's list last gave them, for the
  /// person away from it; at a node, Phones gives its records. Kept only
  /// in this run of the app, as a list is asked for when wanted.
  List<PhoneRecord>? get listed {
    final l = _listed, h = me.home;
    return l != null && h != null && l.home == hex(h.id) ? l.records : null;
  }

  ({String home, List<PhoneRecord> records})? _listed;

  void _list(Outgoing o, List<PhoneRecord> records) =>
      _listed = (home: hex(o.dst.id), records: records);

  /// Sends `c` to the home on a fetch, from the node the phone is on, its
  /// answer coming with the mail; what to tell the person.
  Future<String> _command(Command c, {String? about}) async {
    if (me.home == null) return 'No home node';
    final here = session.address;
    if (!session.connected || here == null) {
      return 'Connect to a node first';
    }
    session.fetchNews = null;
    await messenger.fetch(here, command: c, about: about);
    return 'Sent to your home; its answer comes with your mail';
  }

  /// At the home, with no owner, takes the seat: the key written to Home
  /// again takes an empty one (spec "Phone link", Settings).
  Future<String> becomeOwner() async {
    if (!atHome) return 'Not at your home';
    await session.writeHome(me.mailboxKey.extractBytes());
    return isOwner ? 'You are its owner' : 'The node did not take it';
  }

  /// Asks the home for the people whose home it is.
  Future<String> askList() => _command(const ListCommand());

  /// Asks the home to stop keeping the mail of `r`'s person.
  Future<String> removeHome(PhoneRecord r) => _command(RemoveCommand(r.number));

  /// Asks the home to make `r`'s person its owner.
  Future<String> handOver(PhoneRecord r) => _command(HandOverCommand(r.number));

  /// Gives `c` a home at the person's: a key made for them, their tag
  /// there and their card's name go to the home, and once it has them `c`
  /// is sent the home and the key ([_obeyed]). The key is kept for that
  /// home, so that asking again after a lost answer adds no second record.
  Future<String> shareHome(Contact was) async {
    final c = contacts.byKey(was.key) ?? was;
    final h = me.home;
    if (h == null) return 'No home node';
    if (!isOwner) return 'Only your home\'s owner can share it';
    var share = c.share;
    if (share == null || hex(share.home.id) != hex(h.id)) {
      share = (home: h, key: sodium.randombytes.buf(keyLen));
      await contacts.put(c.copyWith(share: share));
    }
    final add = AddCommand(
      share.key,
      recipientTag(sodium, c.card.identity, h.id),
      c.card.name,
    );
    return _command(add, about: c.key);
  }

  /// What the home did with the command of the fetch `o`, which `s`
  /// answers; told to the person once.
  @override
  Future<void> _obeyed(Outgoing o, RoundSummary s) async {
    final bytes = o.command;
    final outcome = s.outcome;
    if (bytes == null || outcome == null) return;
    final Command c;
    try {
      c = Command.decode(bytes);
    } on BadPayload {
      return;
    }
    String? name(int number) {
      final r = listed?.where((r) => r.number == number).firstOrNull;
      return r == null || r.name.isEmpty ? null : r.name;
    }

    final String? news;
    switch ((c, outcome)) {
      case (_, Outcome.notAllowed):
        news = 'Your home says you are not its owner';
      case (AddCommand(), Outcome.ok || Outcome.already):
        final to = contacts.byKey(o.to);
        final share = to?.share;
        if (to == null || share == null) {
          news = null;
        } else {
          await messenger.sendHome(to, share);
          news = '${to.name} has a home at yours now. They are told';
        }
      case (AddCommand(), Outcome.full):
        news = 'Your home is full: it keeps eight people';
      case (ListCommand(), Outcome.ok):
        _list(o, s.list ?? const []);
        news = null;
      case (RemoveCommand(:final number), Outcome.ok):
        news = 'Removed from your home';
        if (listed case final l?) {
          _list(o, [...l.where((r) => r.number != number)]);
        }
      case (HandOverCommand(:final number), Outcome.ok):
        news = switch (name(number)) {
          null => 'Your home has a new owner',
          final n => '$n is your home\'s owner now',
        };
        if (listed case final l?) {
          _list(o, [for (final r in l) r.copyWith(owner: r.number == number)]);
        }
      case (RemoveCommand() || HandOverCommand(), Outcome.noSuchHome):
        news = 'Already gone';
      default:
        news = null;
    }
    session.note('Command ${c.runtimeType}: ${outcome.name}');
    if (news != null) session.fetchNews = news;
    notifyListeners();
  }
}
