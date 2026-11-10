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

  /// The card changed and has not gone to the contacts. Without contacts
  /// there is no one to send it to.
  bool get cardUnsent => me.cardUnsent && contacts.list.isNotEmpty;

  /// Why the card changed, for the line that offers to send it.
  String get cardChange => me.home == null
      ? 'Your card changed: no home'
      : 'Your card changed: new home address';

  /// The connected node is the person's home.
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

  static const _oldHomeKeeps =
      'Your old home keeps your mail until you stop it there.';

  /// Makes the connected node, at `a`, the person's home: it is given the
  /// mailbox key and is the home only if it then says it holds one. The
  /// new card goes to every contact if its address changed. Returns what
  /// to tell the person.
  Future<String> makeHome(Address a) async {
    final n = session.node;
    if (n == null) return 'Not connected to a node';
    final h = await session.writeHome(me.keys.mailbox.extractBytes());
    if (!h.mine) {
      return 'The node refused. Nothing changed.';
    }
    final away = me.home != null && !atHome;
    if (await me.setHome(a)) await messenger.sendCardToAll();
    return away
        ? 'This node is your home. $_oldHomeKeeps'
        : 'This node is your home';
  }

  /// No home node. Connected to the home, it forgets the mailbox key;
  /// elsewhere the home keeps it, and the person is told.
  Future<String> noHome() async {
    if (me.home == null) return 'No home node';
    final at = atHome;
    final h = at ? await session.writeHome(null) : null;
    if (h != null && h.mine) {
      return 'The node refused. It is still your home.';
    }
    if (await me.setHome(null)) await messenger.sendCardToAll();
    return at ? 'No home node' : 'No home node. $_oldHomeKeeps';
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
