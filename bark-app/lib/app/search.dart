// The search: the phone's situation, the loop that decides on it
// (`decision.dart`), carries the decision out and feeds the round's
// result back, and what the screens say of it.

part of '../app.dart';

mixin _Search on _AppCore {
  /// Whether the search has started, which only the app on a phone does:
  /// a test starts it to drive it.
  bool _searchStarted = false;

  /// Bluetooth is on, by Android's word.
  bool bluetoothOn = false;
  StreamSubscription<AvailabilityState>? _availability;

  /// The search's loop runs.
  bool _searching = false;

  /// The scan under way, which ends once it has stopped.
  Future<void>? _round;

  /// Completed to end the search's scan or wait at once.
  Completer<void> _wake = Completer<void>();

  /// What the search did when last asked ([modeOf]); a change wakes it.
  Mode _mode = Mode.idle;

  /// The search's rounds in a row that have reached none of the paired
  /// nodes.
  int _missed = 0;

  /// When the search last started a scan, a round's or a look-around's.
  DateTime? _scanned;

  /// The link the search last saw, and when the phone last lost one it
  /// held between rounds ([Situation.droppedAt]).
  LinkNow? _had;
  DateTime? _dropped;

  /// What the last whole round heard.
  List<FoundNode> _lastRound = const [];

  /// The last failure the search noted, so that a failure every round
  /// is told once.
  String? _searchFailed;

  /// The nodes this phone is paired with that still hold its bond; one
  /// that no longer does is the person's to decide on, with Pair again.
  Set<String> get pairedNodes => nodes.own;

  /// The home's node, by name, if one is set.
  String? get homeName {
    final h = me.home;
    return h == null ? null : nodeName(h.id);
  }

  /// The paired node the search looks for first: the home when the phone
  /// is paired with it, else the one connected last; of those it may use
  /// ([NodeRecord.usable]).
  String? get firstPaired {
    final p = nodes.usable;
    final h = homeName;
    return h != null && p.contains(h) ? h : p.lastOrNull;
  }

  /// The phone's situation, on which the search decides ([decide]), with
  /// what a scan just `heard`.
  @override
  Situation situation({List<FoundNode>? heard}) => Situation(
    now: now(),
    home: homeName,
    nodes: nodes.records,
    link: _linkNow,
    linking: connecting || _unpairing,
    radio: bluetoothAllowed && bluetoothOn && !scanning,
    stopped: session.stopped,
    awaited: _awaited,
    missed: _missed,
    busy: !settled,
    toSend: session.toSend,
    scannedAt: _scanned,
    droppedAt: _dropped,
    heard: heard,
  );

  /// Starts the search, which then runs whenever it should: at start, and
  /// after a link drops.
  void startSearch() {
    if (_searchStarted) return;
    _searchStarted = true;
    _availability = UniversalBle.availabilityStream.listen((a) {
      bluetoothOn = a == AvailabilityState.poweredOn;
      notifyListeners();
      unawaited(checkBluetooth());
    });
    unawaited(checkBluetooth());
    notifyListeners();
  }

  /// Runs the search when it should, and ends a scan or wait under way
  /// when what it does changes: a link that drops starts a round, and one
  /// that comes up ends the round's wait.
  void _search() {
    if (!_searchStarted) return;
    final l = _linkNow;
    final had = _had;
    // An errand's link is the round's own, and its end no news.
    if (l == null && had != null && (!had.guest || had.byTap)) {
      _dropped = now();
    }
    _had = l;
    final m = modeOf(situation());
    if (m != _mode) {
      _mode = m;
      if (!_wake.isCompleted) _wake.complete();
    }
    if (m == Mode.idle || _searching) return;
    _searching = true;
    unawaited(_run());
  }

  /// The search: decides, carries the decision out, and decides again on
  /// what came of it, until there is nothing to do.
  Future<void> _run() async {
    try {
      List<FoundNode>? heard;
      while (_searchStarted) {
        if (_wake.isCompleted) _wake = Completer<void>();
        var s = situation(heard: heard);
        var d = decide(s);
        if (d case GiveUp(:final node)) {
          _giveUp(node);
          s = situation(heard: heard);
          d = decide(s);
        }
        heard = null;
        switch (d) {
          case Idle():
            // A scan cut short by a change heard too little to judge: the
            // next round comes as soon as the search runs again.
            if (s.heard != null) _scanned = null;
            return;
          case Wait(:final wait):
            await _sleep(wait);
          case Listen(:final until):
            heard = await _listenRound(until);
          case Connect(:final node):
            await _connectFound(node);
          case Errand(:final node):
            await _connectPublic(node);
          case MoveHome(:final node):
            session.note(
              'Heard ${node.name}, ${node.rssi ?? '?'} dBm: moving to it',
            );
            await _connectFound(node);
          case GiveUp():
            throw StateError('decided to give up twice');
        }
        if (!_searchStarted) return;
        if (s.heard case final round?) _roundDone(round, s);
      }
    } finally {
      _searching = false;
    }
  }

  /// Waits `d`, or until [_wake] completes.
  Future<void> _sleep(Duration d) async {
    final t = Timer(d, () {
      if (!_wake.isCompleted) _wake.complete();
    });
    await _wake.future;
    t.cancel();
  }

  /// A scan of one round, ended early once `until` is heard well; then
  /// Android's bond with each node heard that the app does not count as
  /// its own goes ([_forgetBonds]), before anything is done with them.
  Future<List<FoundNode>> _listenRound(String? until) async {
    _scanned = now();
    final round = _listen(
      until == null ? null : (n) => n.name == until && n.usable,
    );
    _round = round;
    final heard = await round;
    _round = null;
    await recheckKept();
    await _forgetBonds(heard.values);
    return heard.values.toList();
  }

  /// What the round that heard `heard`, decided on in `s`, leaves behind:
  /// a miss of the paired nodes where it left the phone on none of them
  /// ([countsMiss]), and the public node in use no longer, once a round
  /// does not hear it.
  void _roundDone(List<FoundNode> heard, Situation s) {
    _lastRound = heard;
    final g = nodes.guest;
    if (g != null && !heard.any((h) => h.name == g)) nodes.guest = null;
    if (!countsMiss(s) || (session.connected && !session.guest)) return;
    _missed++;
    if (_missed == ownGone) {
      session.note(
        'Not reached ${s.own.join(', ')} in $ownGone rounds: out of reach, '
        'using public nodes as a guest',
      );
    }
    notifyListeners();
  }

  @override
  void _heard(Iterable<FoundNode> heard) {
    for (final n in heard) {
      nodes.heard(n, now());
    }
  }

  Future<Map<String, FoundNode>> _listen(
    bool Function(FoundNode)? until,
  ) async {
    try {
      final heard = await listen(until: until, stop: _wake.future);
      _heard(heard.values);
      _searchFailed = null;
      return heard;
    } catch (e) {
      final f = 'Looking for nodes failed: $e';
      if (f != _searchFailed) session.note(f);
      _searchFailed = f;
      return const {};
    }
  }

  /// The phone reached a node of its own, or gave one up: the rounds
  /// that missed the paired nodes, and the public node used meanwhile,
  /// bear on what comes next no more.
  @override
  void _roundsAfresh() {
    if (ownAway) session.note('Back on a paired node');
    _missed = 0;
    nodes.guest = null;
  }

  /// Ends the search's scan and waits until it has stopped, so that a
  /// scan or connect of the person's does not meet it.
  @override
  Future<void> _searchStopped() async {
    if (!_wake.isCompleted) _wake.complete();
    await _round;
  }

  // What the screens say of the search.

  /// Whether the search runs, as the person reads it: not once they
  /// stopped it, nor on a node of theirs, nor on a guest's link they made
  /// by a tap ([tappedGuest]), nor while unpairing.
  bool get searchOn {
    if (!_searchStarted || session.stopped || _unpairing) return false;
    final l = _linkNow;
    return l == null || (l.guest && !l.byTap);
  }

  /// The paired node the search looks for first, while it runs
  /// ([firstPaired]); null without one, when it looks for public nodes
  /// alone.
  String? get searchFor => searchOn ? firstPaired : null;

  /// Whether the search scans now: not before Bluetooth is allowed, nor
  /// while it is off, a connection is being made, or the person scans.
  bool get lookingNow =>
      searchOn && bluetoothAllowed && bluetoothOn && !connecting && !scanning;

  /// The phone, on a paired node, moves to the home once it hears it
  /// ([homeAbove]).
  bool get movesHome => _searchStarted && homeAbove(situation()) != null;

  /// The last round heard a node of the phone's own, too weak to take
  /// ([ownPassedOver]), and the phone uses a public node meanwhile.
  bool get ownWeak =>
      !(session.connected && !session.guest) &&
      ownPassedOver(situation(heard: _lastRound));

  /// Whether the link in use may be closed with nothing lost: nothing in
  /// flight on it ([Session.inFlight]) and no firmware update under way
  /// ([Updater.underWay]).
  bool get settled => !session.inFlight && !updater.underWay;

  /// The paired node the search is scanning for first now.
  String? get lookingFor => lookingNow ? firstPaired : null;

  /// The paired nodes are out of reach: [ownGone] rounds in a row have
  /// reached none the search may use, and the phone uses public nodes as a
  /// guest meanwhile.
  bool get ownAway => nodes.usable.isNotEmpty && _missed >= ownGone;

  /// The public node the phone uses as a guest, between its errands too:
  /// the node of the last, while the search runs; or the node of a guest's
  /// link the person made by a tap, while it is up.
  String? get guestNode {
    final l = _linkNow;
    if (l != null && l.guest && l.byTap) return l.node;
    return searchOn ? nodes.guest : null;
  }

  /// The link up is a guest's the person made by a tap, which the node
  /// ends after a guest's 30 s (spec "Phone link", Public).
  bool get tappedGuest {
    final l = _linkNow;
    return l != null && l.guest && l.byTap;
  }

  /// The record of [guestNode], once an errand there has read it, for
  /// making it the home or pairing with it between errands.
  @override
  NodeRecord? get guestHere {
    final g = guestNode;
    final r = g == null ? null : nodes[g];
    return r?.asGuest != null && r?.heard != null ? r : null;
  }

  /// The person stops the search for the paired node, until they connect.
  @override
  Future<void> stopLooking() => session.setStopped(true);
}
