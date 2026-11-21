// The person's home and card: the home's mailbox key, the fetch from
// it, the card's addresses, and the offer of a guest's node as the home
// (spec "Mailbox"; roadmap D165).

part of '../app.dart';

/// What a person reads of a node that keeps as many phones as it can and
/// so takes no more pairings (spec "Phone link", Pairing).
const String fullNode = 'This node is full: eight phones are paired with it.';

mixin _Home on _AppCore {
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

  /// Sets the person's name, which their card carries and the node they
  /// are paired with lists for its other phones.
  Future<void> setName(String n) async {
    await me.setName(n);
    await session.nameChanged();
  }

  /// The card changed and has not gone to the contacts. Without contacts
  /// there is no one to send it to.
  bool get cardUnsent => me.cardUnsent && contacts.list.isNotEmpty;

  /// Why the card changed, for the line that offers to send it.
  String get cardChange => me.home == null
      ? 'Your card changed: no home'
      : 'Your card changed: new home address';

  /// The connected node is the person's home.
  @override
  bool get atHome {
    final n = session.node;
    return n != null && messenger.isHome(n.id);
  }

  /// The connected node holds a mailbox key the phone can clear and is
  /// not the person's home: a former home of theirs.
  bool get holdsOtherKey {
    final h = session.nodeHome;
    return !atHome && h != null && h.mine;
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
  @override
  Future<void> _sendQueuedFetch() =>
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
  void _queuedFetch() => unawaited(_sendQueuedFetch());

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

  /// No home node. Connected to the home, it forgets the mailbox key;
  /// elsewhere the home is left ([_leave]).
  Future<String> noHome() async {
    if (me.home == null) return 'No home node';
    final at = atHome;
    final h = at ? await session.writeHome(null) : null;
    if (h != null && h.mine) {
      return 'The node refused. It is still your home.';
    }
    final left = !at && await _leave();
    if (await me.setHome(null)) await messenger.sendCardToAll();
    return left ? 'No home node. $_oldHomeLets' : 'No home node';
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
    _leaveLater();
  });

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

  /// Takes the mailbox key from a connected node that is not the home.
  Future<String> clearKey() async {
    final h = await session.writeHome(null);
    return h.mine
        ? 'The node refused. It still keeps your mail.'
        : 'It no longer keeps your mail';
  }

  /// The node the phone uses as a guest is the person's home, as after
  /// they unpaired from their home: the node keeps the mailbox key.
  bool get guestAtHome {
    final a = guestHere?.asGuest?.address;
    return a != null && messenger.isHome(a.id);
  }

  /// Whether the first screen offers the public node the phone uses as a
  /// guest ([guestHere]) as the home, while none is set, once it has a
  /// place and if it can be a home and takes a pairing. Not again in the
  /// app's run once the person said not now.
  bool get offerHome {
    if (_homeNotNow || me.home != null) return false;
    final g = guestHere?.asGuest;
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
    final g = guestHere;
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
}
