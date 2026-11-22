// The person's home and its owner (spec "Mailbox"; "Phone link", The
// node's owner): the home's mailbox key, the fetch from it, the leave of
// the home before, the offer of a guest's node as the home (roadmap
// D165), and the owner's commands on a fetch from anywhere.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';

import 'app_layer.dart';
import 'ble_link.dart';
import 'contacts.dart';
import 'link.dart';
import 'me.dart';
import 'messenger.dart';
import 'nodes.dart';
import 'session.dart';

/// What a person reads of a node that keeps as many phones as it can and
/// so takes no more pairings (spec "Phone link", Pairing).
const String fullNode = 'This node is full: eight phones are paired with it.';

class Mailbox extends ChangeNotifier {
  final Sodium sodium;
  final Session session;
  final Me me;
  final Contacts contacts;
  final Messenger messenger;
  final DateTime Function() now;

  /// The public node the phone uses as a guest, once an errand has read
  /// it (`Links.guestHere`).
  final NodeRecord? Function() guestHere;

  /// Connects to a node as the person's tap does, pairing with it
  /// (`Links.connect`).
  final Future<void> Function(FoundNode) connect;

  Mailbox({
    required this.sodium,
    required this.session,
    required this.me,
    required this.contacts,
    required this.messenger,
    required this.now,
    required this.guestHere,
    required this.connect,
  }) {
    messenger
      ..summarised = _answered
      ..itemCame = _itemCame;
    session.addListener(_sessionChanged);
  }

  void _sessionChanged() {
    _nodeAddress();
    unawaited(sendQueuedFetch());
    _ownerSeen();
    _leftHere();
    // Leaving a home waits for a node with a place for the answers.
    _leaveNow();
  }

  @override
  void dispose() {
    session.removeListener(_sessionChanged);
    _leaveTimer?.cancel();
    super.dispose();
  }

  /// Keeps the node's address for the card, and the home's.
  void _nodeAddress() {
    final a = session.address;
    if (a == null || !session.connected) {
      // The link ended: the card says when it was last up.
      if (!session.connected && me.linked) unawaited(me.unlinked());
      return;
    }
    if (!me.linked || a.toString() != me.nodeAddress?.toString()) {
      unawaited(me.setNodeAddress(a));
    }
    // The home is the node, not the spot: a fixed, awake node's address is
    // its stored place, which moves only at boot or when it turns fixed,
    // and the card and texts to the old one would miss it. A carried
    // node's address is its live fix, which moves every second, and would
    // change the card as often; a sleepy node's is its anchor's, which
    // changes as it picks anchors. The changed card waits for the person to
    // send it.
    final s = session.status;
    if (s == null || s.carried || s.sleepy) return;
    if (atHome && a.toString() != me.home.toString()) unawaited(me.setHome(a));
  }

  /// The connected node is the person's home.
  bool get atHome {
    final n = session.node;
    return n != null && messenger.isHome(n.id);
  }

  /// The connected node holds a mailbox key the phone can clear, is not
  /// the person's home and is not one the app is leaving by itself
  /// ([_leftHere]): the app does not know why it holds it, so the person
  /// says whether it forgets them.
  bool get holdsOtherKey {
    final h = session.nodeHome, n = session.node;
    final leaving =
        n != null && hex(me.leaving?.home.id ?? const []) == hex(n.id);
    return !atHome && h != null && h.mine && !leaving;
  }

  /// The connected node, not the person's home, is someone else's. Only
  /// its keeper, or a removal by the code on its screen, takes that key.
  bool get othersHome {
    final h = session.nodeHome;
    return !atHome && h != null && !h.mine && h.set;
  }

  /// The person's home does not hold their mailbox key now: a phone
  /// paired with it may have removed it ([Session.homeRemoved]).
  bool get homeKeyGone =>
      atHome && session.nodeHome?.mine == false && session.homeRemoved;

  /// Fetches to the node's address; returns what to tell the person. Only
  /// the person's word fetches, as fetched mail crosses the city twice
  /// (roadmap D128).
  Future<String> fetchNow() async {
    if (me.home == null) return 'No home node to fetch from';
    if (!session.connected) {
      await me.setFetchQueued(true);
      return 'The fetch waits for a node';
    }
    if (atHome) {
      return 'At your home, your mail comes by itself';
    }
    if (session.fetchWaiting != null) {
      return 'Still waiting for the last fetch';
    }
    final here = session.address;
    if (here == null) {
      return 'This node has no place yet, so mail cannot come back to it';
    }
    session.fetchNews = null;
    await messenger.fetch(here);
    return 'Fetching your mail';
  }

  /// Sends the queued fetch once a node is linked and has an address,
  /// from it; at the home there is nothing to fetch, as the mail is in
  /// its inbox here.
  Future<void> sendQueuedFetch() =>
      _fetchSend ??= _sendFetch().whenComplete(() => _fetchSend = null);

  Future<void> _sendFetch() async {
    if (!me.fetchQueued || !session.connected) return;
    if (atHome) {
      await me.setFetchQueued(false);
      return;
    }
    final here = session.address;
    if (here == null || session.fetchWaiting != null || me.home == null) {
      return;
    }
    await me.setFetchQueued(false);
    session.fetchNews = null;
    await messenger.fetch(here);
    session.note('Fetch: sent, queued while no node was linked');
  }

  /// The queued fetch being sent, which a guest's errand waits for.
  Future<void>? _fetchSend;

  static const _oldHomeLets =
      'Your old home sends you its mail, then forgets you.';

  /// Makes the connected node, at `a`, the person's home: it is given the
  /// mailbox key and is the home only if it then says it holds one. The
  /// home before is left ([_leave]). The new card goes to every contact
  /// if its address changed. Returns what to tell the person.
  Future<String> makeHome(Address a) async {
    final n = session.node;
    if (n == null) return 'Not connected to a node';
    final h = await session.writeHome(me.keys.mailbox.extractBytes());
    if (!h.mine) {
      return 'The node refused. Nothing changed.';
    }
    final left = !atHome && await _leave();
    if (await me.setHome(a)) await messenger.sendCardToAll();
    _ownerSeen();
    return left
        ? 'This node is your home. $_oldHomeLets'
        : 'This node is your home';
  }

  /// Removes the home. Connected to the home, it forgets the mailbox key;
  /// elsewhere the home is left ([_leave]).
  Future<String> noHome() async {
    if (me.home == null) return 'No home';
    final at = atHome;
    final h = at ? await session.writeHome(null) : null;
    if (h != null && h.mine) {
      return 'The node refused. It is still your home.';
    }
    final left = !at && await _leave();
    if (await me.setHome(null)) await messenger.sendCardToAll();
    return left ? 'Home removed. $_oldHomeLets' : 'Home removed';
  }

  /// Makes the home that `c`, its owner, gave the person theirs, with the
  /// key they were given; the home before is left ([_leave]). A fetch
  /// from the node the phone is on, or the next one, shows the home holds
  /// the key. Nothing goes back to `c`.
  Future<String> useGift(Contact was) async {
    final c = contacts.byKey(was.key) ?? was;
    final g = c.gift;
    if (g == null) return 'No home to use';
    final left =
        me.home != null && hex(me.home!.id) != hex(g.home.id) && await _leave();
    if (await me.setHome(g.home, key: g.key)) await messenger.sendCardToAll();
    await contacts.put(c.copyWith(gift: null));
    final here = session.connected ? session.address : null;
    if (here == null) {
      await me.setFetchQueued(true);
    } else {
      session.fetchNews = null;
      await messenger.fetch(here);
    }
    final home = '${nodeLabel(g.home.id)} is your home';
    return left ? '$home. $_oldHomeLets' : home;
  }

  /// The person declines the home `c` gave them.
  Future<void> declineGift(Contact c) => contacts.put(c.copyWith(gift: null));

  /// The person's home is `a`, they say, as after their identity came
  /// back from its words: a fetch to it with the key their identity makes,
  /// from the node the phone is on, makes it the home once its summary
  /// comes ([_answered]); without one nothing changes.
  Future<String> tryHome(Address a) async {
    if (me.home != null) return 'You have a home already';
    final here = session.connected ? session.address : null;
    if (here == null) return 'Connect to a node first';
    session.fetchNews = null;
    await messenger.fetch(here, to: a);
    return 'Asking ${nodeLabel(a.id)} for your mail';
  }

  /// The home the person named last ([tryHome]) while it has not
  /// answered, and whether its wait is over: it did not answer.
  ({Address home, bool over})? get tried {
    if (me.home != null) return null;
    final o = session.outbox.where((o) => o.kind == OutKind.fetch).lastOrNull;
    final l = me.leaving?.home;
    if (o == null || o.answered || o.command != null) return null;
    if (l != null && hex(l.id) == hex(o.dst.id)) return null;
    return (home: o.dst, over: session.fetchWaiting != o);
  }

  /// A summary of the fetch of `round` came: from the home being left, of
  /// a home the person named, or of a command.
  Future<void> _answered(int round, RoundSummary s) async {
    final l = me.leaving;
    if (l != null && (round == l.round || round == l.leaveRound)) {
      return _leavingAnswered(l, round, s);
    }
    final o = session.outbox
        .where((o) => o.kind == OutKind.fetch && o.sent == round)
        .firstOrNull;
    if (o == null) return;
    if (s.outcome == Outcome.removed) return _homeRemoved(o);
    if (me.home == null && o.command == null) {
      if (await me.setHome(o.dst)) await messenger.sendCardToAll();
      // Its summary is now the home's: what it keeps, its owner, what it
      // reads.
      await me.gotSummary(round, s);
      session.note('Home: ${nodeName(o.dst.id)} answered, now the home');
      session.fetchNews = '${nodeLabel(o.dst.id)} is your home';
      notifyListeners();
      return;
    }
    await _obeyed(o, s);
  }

  /// The node the fetch `o` went to says it no longer keeps the person's
  /// mail (spec "Mailbox", The answer). If it is the home, the person has
  /// none now, and the card without it goes to the contacts, as for No
  /// home node; a home they named ([tryHome]) changes nothing.
  Future<void> _homeRemoved(Outgoing o) async {
    final h = me.home;
    session.note('Home: ${nodeName(o.dst.id)} says the home was removed');
    if (h == null || hex(h.id) != hex(o.dst.id)) {
      session.tell(
        '${nodeLabel(o.dst.id)} does not keep your mail. Nothing changed.',
      );
      return;
    }
    if (await me.setHome(null)) await messenger.sendCardToAll();
    session.tell(
      'Your home no longer keeps your mail. Choose a new home in Settings.',
    );
  }

  /// The last fetch to the home had no answer within its wait: the home
  /// is off or out of reach, or has forgotten the person, and Settings
  /// says what they can do.
  bool get homeSilent {
    final h = me.home;
    if (h == null) return false;
    final o = session.outbox
        .where((o) => o.kind == OutKind.fetch && hex(o.dst.id) == hex(h.id))
        .lastOrNull;
    return o != null &&
        !o.answered &&
        o.state == OutState.taken &&
        now().difference(o.at) >= Session.fetchAnswerWait;
  }

  /// Leaves the person's home, which is about to change: the app fetches
  /// the mail it keeps and then tells it to forget the person, by itself
  /// ([_leaveStep]). Whether there was one.
  Future<bool> _leave() async {
    final h = me.home;
    if (h == null) return false;
    final was = me.leaving;
    if (was != null) {
      session.note('Leaving: ${nodeName(was.home.id)} given up for another');
      await me.addLeft(LeftHome(was.home, was.key));
    }
    await me.setLeaving(Leaving(h, me.mailboxKey.extractBytes()));
    session.note('Leaving: ${nodeName(h.id)}');
    unawaited(_leaveStep());
    return true;
  }

  /// The step of leaving under way, one at a time.
  Future<void>? _leaving;

  /// Takes the next step of leaving the old home, once the phone is on a
  /// node with a place for the answers: a fetch, again while the home
  /// keeps more than a batch, once every item of the last has come or its
  /// wait is over; then the leave. The leave's answer may be lost, and a
  /// leave asked again is not answered, as the home no longer holds the
  /// key, so it is sent once and the old home forgotten when its answer
  /// comes or its wait is over.
  Future<void> _leaveStep() => _leaving ??= _stepLeaving().whenComplete(() {
    _leaving = null;
    _leftHere();
    _leaveLater();
  });

  /// On a home left whose leave was not answered, takes the leave up
  /// again if the node, read afresh, still holds the key, once no other
  /// leave is under way: the home was off or out of reach when the person
  /// left. A node that no longer holds the key, or is the home again, is
  /// forgotten as left. Once a connection: a node that holds the key and
  /// does not answer a leave on its own link is not asked over and over.
  void _leftHere() {
    final n = session.node;
    if (n == null || !session.connected) {
      _takenUp = false;
      return;
    }
    if (_readingLeft || _takenUp) return;
    final l = me.leftAt(n.id);
    if (l == null || me.leaving != null) return;
    _readingLeft = true;
    unawaited(
      _takeUp(l)
          .whenComplete(() => _readingLeft = false)
          .catchError((Object e) {
            session.note('Leaving: $e');
          }),
    );
  }

  bool _readingLeft = false;
  bool _takenUp = false;

  Future<void> _takeUp(LeftHome l) async {
    final h = await session.readHomeAgain();
    if (h == null || me.leaving != null) return;
    final holds = h.mine && !messenger.isHome(l.home.id);
    await me.dropLeft(l.home.id);
    if (!holds) return;
    _takenUp = true;
    session.note('Leaving: ${nodeName(l.home.id)} still holds the key');
    await me.setLeaving(Leaving(l.home, l.key));
    unawaited(_leaveStep());
  }

  /// Wakes the leave once the wait of its fetch or of the leave is over.
  Timer? _leaveTimer;

  /// Takes the next step of leaving, if there is one to take now.
  void _leaveNow() {
    if (me.leaving != null) unawaited(_leaveStep());
  }

  /// Takes the next step once the wait of the step under way is over; a
  /// wait already over waits for the phone to be on a node instead.
  void _leaveLater() {
    _leaveTimer?.cancel();
    final l = me.leaving;
    final at = l?.leaveAt ?? l?.at;
    if (at == null) return;
    final wait = at.add(Session.fetchAnswerWait).difference(now());
    if (wait > Duration.zero) _leaveTimer = Timer(wait, _leaveNow);
  }

  Future<void> _stepLeaving() async {
    final l = me.leaving;
    if (l == null) return;
    final t = now();
    bool over(DateTime? at) =>
        at != null && t.difference(at) >= Session.fetchAnswerWait;
    if (l.leaveRound != null) {
      if (!over(l.leaveAt)) return;
      session.note('Leaving: ${nodeName(l.home.id)} did not answer the leave');
      // On the node itself it is not taken up again in this connection.
      final n = session.node;
      if (n != null && hex(n.id) == hex(l.home.id)) _takenUp = true;
      await me.addLeft(LeftHome(l.home, l.key));
      await me.setLeaving(null);
      return;
    }
    final here = session.connected ? session.address : null;
    if (here == null) return;
    final got = l.items != null && l.got >= l.items!;
    final fetch = l.round == null || (got && l.remaining > 0);
    if (!fetch && !got && !over(l.at)) return;
    final key = SecureKey.fromList(sodium, l.key);
    try {
      if (fetch) {
        l.round = await messenger.fetchFrom(l.home, key, here);
        l.at = t;
        l.items = null;
        l.got = 0;
      } else {
        l.leaveRound = await messenger.fetchFrom(
          l.home,
          key,
          here,
          command: const LeaveCommand(),
        );
        l.leaveAt = t;
      }
    } finally {
      key.dispose();
    }
    session.note(
      'Leaving: ${fetch ? 'a fetch' : 'the leave'} to ${nodeName(l.home.id)}',
    );
    await me.leavingChanged();
  }

  Future<void> _leavingAnswered(Leaving l, int round, RoundSummary s) async {
    // A home that says it was removed has forgotten the person already.
    if (round == l.leaveRound || s.outcome == Outcome.removed) {
      session.note('Leaving: ${nodeName(l.home.id)} forgot the person');
      await me.setLeaving(null);
      return;
    }
    l.items = s.items;
    l.remaining = s.remaining;
    await me.leavingChanged();
    unawaited(_leaveStep());
  }

  /// An item of the fetch of `round` came: one more of the old home's.
  void _itemCame(int round) {
    final l = me.leaving;
    if (l == null || round != l.round) return;
    l.got++;
    unawaited(me.leavingChanged().then((_) => _leaveStep()));
  }

  /// Takes the mailbox key from a connected node that is not the home,
  /// on the person's word ([holdsOtherKey]).
  Future<String> clearKey() async {
    final h = await session.writeHome(null);
    return h.mine
        ? 'The node refused. It still keeps your mail.'
        : 'It forgot you';
  }

  /// The node the phone uses as a guest is the person's home, as after
  /// they unpaired from their home: the node keeps the mailbox key.
  bool get guestAtHome {
    final a = guestHere()?.asGuest?.address;
    return a != null && messenger.isHome(a.id);
  }

  /// Whether the first screen offers the public node the phone uses as a
  /// guest ([guestHere]) as the home, while none is set, once it has a
  /// place and if it can be a home and takes a pairing. Not again in the
  /// app's run once the person said not now.
  bool get offerHome {
    if (_homeNotNow || me.home != null) return false;
    final g = guestHere()?.asGuest;
    return g != null && g.address != null && g.homeable && g.room != 0;
  }

  /// The person declined the offer of the home in this run of the app.
  bool _homeNotNow = false;

  /// The person declines the offer of the home.
  void notHome() {
    _homeNotNow = true;
    notifyListeners();
  }

  /// Makes the node the phone is on the person's home ([makeHome]). A
  /// public node it uses as a guest is paired with first, by the code its
  /// screen shows ([connect]), as only the paired phone may give a node
  /// the mailbox key (spec "Phone link", Pairing); a pairing that fails
  /// throws, and the phone goes on using the node as a guest. Returns what
  /// to tell the person.
  Future<String> homeHere() async {
    final g = guestHere();
    if (g != null && g.asGuest!.room == 0) return fullNode;
    if (g != null) await connect(g.heard!);
    final s = session;
    final a = s.address;
    if (!s.connected || s.guest || a == null) {
      return 'Not paired, so not your home';
    }
    if (!canBeHome(s.status)) {
      return 'Paired, but a home must stay on power in one place';
    }
    return makeHome(a);
  }

  // The owner.

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
