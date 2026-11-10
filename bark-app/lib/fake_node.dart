// A node in Dart that answers as the spec's "Phone link" says, so the
// app can be tried on a phone with no board. Its network is invented:
// it hears relays, gives some messages back, and behind it is one
// person, the fake friend, who writes first and echoes every text sent
// to them, sealed as a real phone would. The fake friend has a home and
// writes from a node away from it, which moves between their messages.
// Somewhere away is the person's home node, whose mailbox answers a
// fetch as the spec's "Mailbox" says. Around it stand a few neighbours,
// one of each link quality and of each kind a map draws apart, all but
// one answering an ask with a status of their own, though a share of
// asks is lost as on a loaded channel. It takes
// a firmware update as the node does, and comes back from its reboot
// running the new version, which hears no one unless a test says so, so
// that the app's warning of a node that hears no one can be seen. Public
// unless its owner makes it private, it also takes guests' links, as the
// spec's "Phone link", Public, says.

import 'dart:async';
import 'dart:math';
import 'dart:typed_data';

import 'package:crypto/crypto.dart' show sha256;
import 'package:sodium/sodium.dart';

import 'app_layer.dart';
import 'card.dart';
import 'link.dart';
import 'node_link.dart';

class FakeNode implements NodeLink {
  final DateTime Function() now;
  final Sodium sodium;

  /// The identity of the app it serves, whom the fake friend writes to.
  final Uint8List app;

  /// The mailbox key of the person's home away, if they have one.
  final SecureKey? mailbox;

  /// What the home away holds, item n at n − 1: one message the fake
  /// friend left there.
  /// The numbers of the home's items it has sent to a fetch.
  final Set<int> _homeSent = {};

  late final List<Uint8List> homeRing = [
    _intro('Left at your home node while you were away.').payload,
  ];

  /// How long after taking a fetch its answer arrives.
  static const fetchAnswerAfter = Duration(seconds: 5);

  /// The fake friend's keys, the same at every start.
  late final Keys friend = Keys.fromSeed(
    sodium,
    List.generate(seedLen, (i) => 0xa0 + i),
  );

  /// The fake friend's card, with their home.
  late final PersonCard friendCard = PersonCard(
    friend.identity,
    friendHome,
    0,
    'Fake friend',
  );

  /// The fake friend's home node, `bark-fa4e00000002`.
  static final friendHome = Address(Position.fromDegrees(50.47, 30.50), [
    0xfa, 0x4e, 0, 0, 0, 2, //
  ]);

  /// The node the fake friend carries, `bark-fa4e00000003`, a block
  /// further north, about 110 m, at each of their messages.
  Address get friendAway => Address(
    Position.fromDegrees(50.40 + 0.001 * _friendWrote, 30.60),
    [0xfa, 0x4e, 0, 0, 0, 3],
  );

  /// `bark-fa4e00000001`.
  static final id = Uint8List.fromList([0xfa, 0x4e, 0, 0, 0, 1]);

  /// Its ID: [id], unless a test's other node has its own.
  Uint8List get nodeId => id;

  /// The fake's GPS fix.
  static final gps = Position.fromDegrees(50.45, 30.52);

  /// A relay's first rotation (spec "Channel at boot").
  static const firstRotation = Duration(seconds: 195);

  /// Relays the fake hears, spread over its first rotation.
  static const relaysHeard = 3;

  /// The fake's noise floor, the spec's figure (spec "Physical layer").
  static const floorDbm = -118;

  /// The neighbour table by slot, as the node keeps it, each with its
  /// age at the fake's start; a test may add more. The slots leave gaps,
  /// as a table does once some have left.
  final Map<int, Neighbour> neighbours = {
    0: _neighbour(1, 300, 0, -700, 4, proven: true),
    3: _neighbour(2, -150, 400, -1125, 9),
    7: _neighbour(3, -500, -250, -1150, 2, demoted: true),
    12: _neighbour(4, 80, -120, -950, 40, sleepy: true, mapPoint: true),
    21: _neighbour(5, -60, 650, -1080, 6, banned: true),
  };

  /// Neighbour `n`, `bark-fa4e000001nn`, `north` and `east` metres from
  /// the fake's GPS fix.
  static Neighbour _neighbour(
    int n,
    double north,
    double east,
    int rssiDdbm,
    int ageS, {
    bool proven = false,
    bool demoted = false,
    bool sleepy = false,
    bool mapPoint = false,
    bool banned = false,
  }) {
    // Metres per degree on the WGS-84 sphere of the mean radius, 6371 km.
    const perDeg = 6371000 * pi / 180;
    final lat = gps.latDeg + north / perDeg;
    final lon = gps.lonDeg + east / (perDeg * cos(gps.latDeg * pi / 180));
    return Neighbour(
      [0xfa, 0x4e, 0, 0, 1, n],
      Position.fromDegrees(lat, lon),
      rssiDdbm: rssiDdbm,
      ageS: ageS,
      proven: proven,
      demoted: demoted,
      sleepy: sleepy,
      mapPoint: mapPoint,
      banned: banned,
    );
  }

  int _neighboursFrom = 0;

  /// What each neighbour answers an ask with, by its ID in hex: one of
  /// each kind its screen names apart. The one left out never answers,
  /// as a neighbour whose question or answer is lost; a test may change
  /// them.
  final Map<String, NeighbourStatus> statuses = {
    _neighbourHex(1): const NeighbourStatus(
      placed: true,
      usb: true,
      heard: 6,
      strong: 4,
      proven: 3,
      version: 9,
      gathers: 10,
      blocks: 31,
      complete: 14,
      have: 40,
    ),
    _neighbourHex(2): const NeighbourStatus(placed: true, heard: 3, strong: 1),
    _neighbourHex(3): const NeighbourStatus(
      placed: true,
      carried: true,
      battery: 82,
      heard: 2,
      version: 8,
    ),
    _neighbourHex(4): const NeighbourStatus(
      placed: true,
      sleepy: true,
      anchored: true,
      battery: 57,
      heard: 4,
      strong: 2,
      proven: 1,
      version: 9,
    ),
  };

  static String _neighbourHex(int n) => hex([0xfa, 0x4e, 0, 0, 1, n]);

  /// The IDs, in hex, of the neighbours asked, in order.
  final List<String> asked = [];

  /// How long after an ask the answer arrives: half the second within
  /// which an answer comes (spec "Phone link", Ask).
  static const answerAfter = Duration(milliseconds: 500);

  /// One ask in this many is lost, the ask or its answer, as on a loaded
  /// channel, where the bench saw 4 answers to 12 asks (2026-11-09); 0
  /// loses none. Counted over the asks a neighbour answers, so a test
  /// knows which; a test may change it.
  int loseEvery = 3;
  int _answerable = 0;

  /// The shortest wait a refusal tells, one ban (spec "Bans").
  static const ban = Duration(milliseconds: 1580);

  /// A frame and the ACK that takes it, the longest hand-over (spec
  /// "Bans"); the fake holds a message this long before its first hop
  /// takes it.
  static const handOver = Duration(microseconds: 155500);

  /// The wait it gives a message back with: 16 bans, told wait code 16
  /// (spec "Bans").
  static const givenBackWait = Duration(milliseconds: 16 * 1580);

  /// Every third message it takes is given back, and every fourth write
  /// finds it not ready, so both paths show.
  static const givenBackEvery = 3;
  static const notReadyEvery = 4;

  /// How long after taking a message the echo arrives.
  static const echoAfter = Duration(seconds: 5);

  late DateTime _boot;
  Position? _mapPoint;

  /// A carried node's address is its live fix, which wanders; a sleepy
  /// node's is not its stored place either.
  bool carried = false;
  bool sleepy = false;

  /// Whether its GPS has a fix; without one and no map point it has no
  /// place. A test may change it.
  bool fixed = true;
  Position _fix = gps;

  /// A new GPS fix, notified as the node does.
  void walkTo(Position p) {
    _fix = p;
    _notify();
  }

  /// The time the phone gave, against the fake's own clock, and the
  /// offset; the fake has no GPS time.
  (int, DateTime)? _time;
  int _offsetMin = 0;

  /// The mailbox key it was given, kept across connections as in flash.
  Uint8List? homeKey;

  /// How often a write changed the key; the same key again changes
  /// nothing, as on the node.
  int homeKeyChanges = 0;
  int _relays = 0;

  /// Its live neighbours as Status counts them, sleepy ones too.
  int _heard = 0;
  bool _ready = true;

  /// The owner's inbox; on a public node, the ring, which never refuses.
  final List<InboxMessage> _inbox = [];

  /// The fake friend's last `sent`, and how many messages they wrote.
  int _friendSent = 0;
  int _friendWrote = 0;
  int _writes = 0;
  int _taken = 0;

  /// The paired phone's link, which this fake is.
  final _Phone _owner = _Phone();
  var _closed = Completer<void>();
  final List<Timer> _timers = [];

  FakeNode({
    required this.now,
    required this.sodium,
    required List<int> app,
    this.mailbox,
  }) : app = Uint8List.fromList(app) {
    _boot = now();
    _hearNeighbours(Duration.zero);
    _place(_intro('Hello from the fake node.'));
    _place(_intro('A second message, waiting in the inbox.'));
  }

  /// The app's recipient tag at this node, which the fake friend's
  /// messages carry.
  late final int _appTag = recipientTag(sodium, app, nodeId);

  Address get _own => Address(_mapPoint ?? _fix, nodeId);

  late final PrecalculatedBox _pair = sodium.crypto.box.precalculate(
    publicKey: app,
    secretKey: friend.pair.secretKey,
  );

  /// A header of the fake friend's next message: their clock, as a phone's
  /// `sent`, and the node they write from, which has moved since the
  /// last, so it is always named.
  Header _header() {
    _friendSent = max(sentOf(now()), _friendSent + 1);
    _friendWrote++;
    return Header(greater(friend.identity, app), _friendSent, friendAway);
  }

  InboxMessage _message(Uint8List p) => InboxMessage(msgId(_own, p), p);

  /// The fake friend's first message, with their card.
  InboxMessage _intro(String text) => _message(
    sealIntro(
      sodium,
      app,
      _appTag,
      writePlain(_header(), [
        CardRecord(friendCard.encode()),
        TextRecord(text),
      ]),
    ),
  );

  /// The text of a message from the app to the fake friend, if it is one,
  /// with its `sent`.
  (int, TextRecord)? _textTo(Uint8List payload) {
    try {
      final plain = switch (formOf(payload)) {
        Form.boxed => openBox(_pair, payload),
        Form.introduction => openIntro(sodium, friend, payload),
        _ => null,
      };
      if (plain == null) return null;
      final (h, rs) = readPlain(plain);
      final t = rs.whereType<TextRecord>().firstOrNull;
      return t == null ? null : (h.sent, t);
    } on BadPayload {
      return null;
    }
  }

  /// The parts of the app's split texts the fake friend has, by the first
  /// part's `sent`.
  final Map<int, List<String?>> _partsIn = {};

  /// The whole text once the fake friend has every part of it.
  String? _whole(int sent, TextRecord t) {
    if (t.of == 1) return t.text;
    final parts = _partsIn[sent - t.part + 1] ??= List.filled(t.of, null);
    parts[t.part - 1] = t.text;
    if (parts.contains(null)) return null;
    _partsIn.remove(sent - t.part + 1);
    return parts.join();
  }

  /// The fake friend's echo of `text`, in parts if it needs them, as the
  /// app splits: the last part first and the rest an [echoAfter] later,
  /// so that a missing part shows.
  void _echo(String text) => _after(echoAfter, () {
    final room = maxAppPayload - boxOverhead - 5 - Address.len - 1;
    final parts = splitText(text, room);
    final n = parts.length;
    if (n > maxParts) return;
    final first = max(sentOf(now()), _friendSent + 1);
    _friendSent = first + n - 1;
    _friendWrote++;
    final sealed = [
      for (final (i, p) in parts.indexed)
        _message(
          sealBox(
            _pair,
            _appTag,
            sodium.randombytes.buf(nonceLen),
            writePlain(
              Header(greater(friend.identity, app), first + i, friendAway),
              [TextRecord(p, i + 1, n)],
            ),
          ),
        ),
    ];
    _place(sealed.last);
    if (n > 1) {
      _after(echoAfter, () => sealed.sublist(0, n - 1).forEach(_place));
    }
  });

  /// The home away's answer to a fetch, as its mailbox gives it: the
  /// newest batch of the items it has not sent, newest first, each with
  /// the count of those still unsent after it, or one reply that says
  /// nothing is new.
  void _answerFetch(Uint8List payload) {
    final key = mailbox;
    if (key == null || formOf(payload) != Form.fetch) return;
    final Fetch f;
    try {
      f = Fetch.open(sodium, key, payload);
    } on BadPayload {
      return;
    }
    final unsent = [
      for (var n = homeRing.length; n >= 1; n--)
        if (!_homeSent.contains(n)) n,
    ];
    final batch = unsent.take(maxReplies).toList();
    _homeSent.addAll(batch);
    final replies = [
      for (final (i, n) in batch.indexed)
        sealReply(
          sodium,
          key,
          f.recipient,
          f.sent,
          i + 1,
          n,
          unsent.length - batch.length,
          homeRing[n - 1],
        ),
    ];
    if (replies.isEmpty) {
      replies.add(sealNothingNew(sodium, key, f.recipient, f.sent));
    }
    _after(fetchAnswerAfter, () {
      for (final r in replies) {
        _place(_message(r));
      }
    });
  }

  /// Learns [relaysHeard] neighbours over the first rotation of a boot
  /// `at` from now: relays, or sleepy nodes alone if `sleepy`.
  void _hearNeighbours(Duration at, {bool sleepy = false}) {
    final step = firstRotation ~/ (relaysHeard + 1);
    final from = _timers.length;
    for (var i = 1; i <= relaysHeard; i++) {
      _after(at + step * i, () {
        _heard++;
        if (!sleepy) _relays++;
      });
    }
    _after(at + firstRotation, () {});
    _learning = _timers.sublist(from);
  }

  /// The timers of the relays being learnt, which a reboot stops.
  List<Timer> _learning = [];

  /// Runs `f` after `d`, then notifies the status.
  void _after(Duration d, void Function() f) {
    _timers.add(
      Timer(d, () {
        f();
        _notify();
      }),
    );
  }

  /// Notifies the status after a test changed what it says.
  void changed() => _notify();

  void _notify() {
    if (_owner.connected) _owner.status.add(_now());
    for (final g in guests) {
      g._phone.status.add(_statusOf(g));
    }
  }

  Status _now() => _statusOf(this);

  /// Status as the phone `p` reads it: on a public node ready for the
  /// phone whose turn it is alone, and a guest's inbox count 1 while a
  /// message waits for it.
  Status _statusOf(Object p) {
    final left = firstRotation - now().difference(_boot);
    final g = p is FakeGuest ? p : null;
    return Status(
      ready: _ready && (!public || _turn == p),
      externalPower: true,
      sleepy: sleepy,
      carried: carried,
      mapPoint: _mapPoint != null,
      placed: fixed || _mapPoint != null,
      booting: left > Duration.zero,
      relays: _relays,
      heard: _heard,
      bootLeftS: _ceilS(left),
      address: _mapPoint ?? _fix,
      inbox: g == null ? _inbox.length : 0,
    );
  }

  static int _ceilS(Duration d) => d <= Duration.zero
      ? 0
      : (d.inMicroseconds + Duration.microsecondsPerSecond - 1) ~/
            Duration.microsecondsPerSecond;

  GivenBack? _givenBackNow([_Phone? p]) {
    final g = (p ?? _owner).givenBack;
    if (g == null) return null;
    return GivenBack(g.$1, _ceilS(g.$2.difference(now())));
  }

  Future<T> _answer<T>(T Function() f) async {
    if (!_owner.connected) throw StateError('not connected');
    return f();
  }

  @override
  Future<NodeInfo> readNode() => _answer(() => _info);

  @override
  Future<void> subscribe() => _answer(() {});

  @override
  Future<Status> readStatus() => _answer(_now);

  @override
  Future<GivenBack?> readGivenBack() => _answer(_givenBackNow);

  @override
  Future<InboxMessage?> readInbox() =>
      _answer(() => _inbox.isEmpty ? null : _inbox.first);

  @override
  Future<void> confirm(int id) => _answer(() {
    if (_inbox.isEmpty || _inbox.first.msgId != id) {
      throw const AttException(AttError.valueNotAllowed);
    }
    _inbox.removeAt(0);
    _notify();
  });

  @override
  Future<void> send(Uint8List value) => _answer(() => _send(this, value));

  /// A Send from the phone `p`, this fake for the owner or a guest.
  void _send(Object p, Uint8List value) {
    if (value.length < Address.len || value.length > Address.len + maxPayload) {
      throw const AttException(AttError.invalidLength);
    }
    // On a public node the turn is Ready: a Send out of turn asks for it,
    // and is answered not ready, as when the node takes none, unless the
    // turn passes to it at once.
    if (public) {
      _ask(p);
      if (_turn != p) {
        _notify();
        throw const AttException(AttError.notReady);
      }
    }
    final phone = _phoneOf(p);
    final dst = Address(
      Position.decode(value),
      value.sublist(Position.wireLen, Address.len),
    );
    final payload = value.sublist(Address.len);
    final id = msgId(dst, payload);
    _writes++;
    if (!_ready || _writes % notReadyEvery == 0) {
      // As when the bucket empties between the status the app read and
      // its write: not ready for one ban.
      if (_ready) {
        _ready = false;
        _since = null;
        _notify();
        _freeAfter(ban);
      }
      throw const AttException(AttError.notReady);
    }
    _taken++;
    _ready = false;
    _since = null;
    if (phone.givenBack != null) {
      phone.givenBack = null;
      if (phone.connected) phone.given.add(null);
    }
    _passTurn();
    _notify();
    if (_taken % givenBackEvery == 0) {
      _after(handOver, () => _giveBack(phone, id, givenBackWait));
      _freeAfter(handOver + givenBackWait);
    } else {
      _freeAfter(handOver);
      if (p != this) return;
      _answerFetch(payload);
      final got = _textTo(payload);
      final text = got == null ? null : _whole(got.$1, got.$2);
      if (text != null) _echo('Echo: $text');
    }
  }

  _Phone _phoneOf(Object p) => p is FakeGuest ? p._phone : _owner;

  void _giveBack(_Phone phone, int id, Duration wait) {
    phone.givenBack = (id, now().add(wait));
    if (phone.connected) phone.given.add(_givenBackNow(phone));
  }

  /// The phones connected, in the order of their links: the owner's link,
  /// this fake, and the guests'.
  final List<Object> _links = [];

  /// The phone whose turn it is on a public node, for which alone Ready is
  /// set; null with no phone connected (spec "Phone link", Sending).
  Object? _turn;

  /// Since when the phone whose turn it is has had Ready.
  DateTime? _since;

  /// The phones whose Recipient said a link version older than
  /// [turnVersion]: they cannot ask, and so want the turn whenever they
  /// lack it.
  final Set<Object> _old = {};

  /// The phones that asked for the turn, by a Send out of turn, and have
  /// not had it since.
  final Set<Object> _asked = {};

  /// Counts the turn's changes, so that a check a ban later sees whether
  /// it still stands.
  int _turnGen = 0;

  bool _wants(Object p) => _asked.contains(p) || _old.contains(p);

  /// The phone the turn goes to from the one that has it, or that stood at
  /// `at` in [_links]: the next that wants it, coming round to the holder
  /// last, else the holder, else the next connected.
  Object? _nextTurn([int? at]) {
    final from = at ?? (_turn == null ? -1 : _links.indexOf(_turn!));
    final order = [
      for (var i = 1; i <= _links.length; i++)
        _links[(from + i) % _links.length],
    ];
    return order.where(_wants).firstOrNull ??
        (_links.contains(_turn) ? _turn : order.firstOrNull);
  }

  bool get _othersWant => _links.any((p) => p != _turn && _wants(p));

  /// Gives the turn to `p`, which wants it no more.
  void _give(Object? p) {
    if (p != null) _asked.remove(p);
    _turn = p;
    _since = _ready && p != null ? now() : null;
    _turnGen++;
    _idleCheck();
  }

  void _passTurn() => _give(_nextTurn());

  /// A phone that has had Ready for a ban while another wants the turn
  /// passes it on; with nobody else wanting it, it keeps it.
  void _idleCheck() {
    final since = _since;
    if (!public || !_ready || since == null || !_othersWant) return;
    final left = since.add(ban).difference(now());
    if (left <= Duration.zero) {
      _passTurn();
      return;
    }
    final gen = _turnGen;
    _after(left, () {
      if (_turnGen == gen) _idleCheck();
    });
  }

  void _joined(Object p) {
    _links.add(p);
    if (_turn == null) _give(p);
  }

  void _gone(Object p) {
    final at = _links.indexOf(p);
    _links.remove(p);
    _old.remove(p);
    _asked.remove(p);
    if (_turn != p) {
      _idleCheck();
      return;
    }
    _turn = null;
    _give(_nextTurn(at - 1));
  }

  /// Recipient from `p` says its app's link version.
  void _app(Object p, int version) {
    if (version < turnVersion) {
      _old.add(p);
    } else {
      _old.remove(p);
    }
    _idleCheck();
  }

  /// A Send from `p`: out of turn, it asks for it, which may pass to it
  /// now.
  void _ask(Object p) {
    if (_turn == p) return;
    _asked.add(p);
    _idleCheck();
  }

  /// Takes messages again after `d`; Ready comes back for the phone
  /// whose turn it is.
  void _freeAfter(Duration d) {
    _after(d, () {
      _ready = true;
      _since = _turn == null ? null : now();
      _turnGen++;
      _idleCheck();
    });
  }

  @override
  Future<void> setMapPoint(Position? p) => _answer(() {
    _mapPoint = p;
    _notify();
  });

  @override
  Future<void> setClock(int utcS, int offsetMin) => _answer(() {
    encodeClock(utcS, offsetMin);
    _time = (utcS, now());
    _offsetMin = offsetMin;
  });

  @override
  Future<NodeTime> readClock() => _answer(() {
    final t = _time;
    if (t == null) return NodeTime(0, _offsetMin, TimeSource.none);
    return NodeTime(
      t.$1 + now().difference(t.$2).inSeconds,
      _offsetMin,
      TimeSource.phone,
    );
  });

  /// Home as the node serves it; the fake has no ring, so its numbers
  /// are 0.
  @override
  Future<void> setHome(List<int>? key) => _answer(() {
    if (key != null && key.length != mailboxKeyLen) {
      throw const AttException(AttError.invalidLength);
    }
    String? held(List<int>? k) => k == null ? null : hex(k);
    if (held(key) != held(homeKey)) {
      homeKey = key == null ? null : Uint8List.fromList(key);
      homeKeyChanges++;
    }
  });

  @override
  Future<NodeHome> readHome() => _answer(
    () => NodeHome(
      homeKey != null || others.any((r) => r.home),
      0,
      0,
      mine: version >= phonesVersion ? homeKey != null : null,
    ),
  );

  @override
  Future<void> setNeighboursFrom(int slot) => _answer(() {
    _neighboursFrom = slot & 0xff;
  });

  /// A page as the node builds it; ages run on from those in the table
  /// and start again every heartbeat interval, 10 s.
  @override
  Future<NeighbourPage> readNeighbours() => _answer(() {
    final slots = neighbours.keys.where((k) => k >= _neighboursFrom).toList()
      ..sort();
    final next = slots.length > NeighbourPage.perPage
        ? slots[NeighbourPage.perPage]
        : 0;
    final up = now().difference(_boot).inSeconds % 10;
    return NeighbourPage(floorDbm, next, _mapPoint ?? _fix, [
      for (final s in slots.take(NeighbourPage.perPage))
        neighbours[s]!.withAge(neighbours[s]!.ageS + up),
    ]);
  });

  /// The firmware image's version it runs.
  int image = 1;

  /// The link version it speaks: this app's, unless a test gives it a
  /// later one, which only adds to what this app reads.
  int version = linkVersion;

  /// On battery at or below the low level: a commit, and a reboot into a
  /// swap, are refused (spec "Phone link", Updates).
  bool lowBattery = false;

  /// For a test: every this many pieces written, one is lost, as the
  /// node's host drops a PDU its full queue has no room for.
  int? dropEvery;
  int _pieces = 0;

  /// For a test: the link is lost once this many pieces are written.
  int? closeAfterPieces;

  /// For a test: the link's end at a reboot overtakes the answer, and
  /// the write fails with Android's catch-all GATT error 133, as on the
  /// bench.
  bool rebootAnswerLost = false;

  /// Pieces the app has written, those lost among them.
  int get piecesWritten => _pieces;

  /// How long its loader takes to swap at a reboot, as a node's does,
  /// 55 s on the bench (`bark-fw/README.md`, "Updates").
  static const swapTime = Duration(seconds: 55);

  /// Whether the image an update brings hears the relays around it. Not
  /// in the demo, so that the person sees what the app says of a node
  /// that hears no one after an update (spec "Phone link", Updates).
  bool hearsAfterUpdate = false;

  /// With [hearsAfterUpdate], the nodes it hears are sleepy alone: it
  /// knows no relay, yet its radio works.
  bool onlySleepyAfterUpdate = false;

  /// The update: the header taken, the bytes held, and whether committed.
  Uint8List? _staging;
  final List<int> _held = [];
  bool _committed = false;
  Refusal? _refused;
  DateTime? _backAt;

  static int _le32(List<int> b, int at) =>
      b[at] | (b[at + 1] << 8) | (b[at + 2] << 16) | (b[at + 3] << 24);

  int get _length => _le32(_staging!, 12);

  /// Guest is a guest's; the owner's link reads its mail through Inbox,
  /// in its own form.
  @override
  Future<void> setGuestAfter(int number) => _answer(() {});

  @override
  Future<GuestItem?> readGuest() async =>
      throw StateError('the paired phone\'s Inbox gives its own form');

  @override
  Future<UpdateProgress> readUpdate() => _answer(() {
    final h = _staging;
    if (h == null) return UpdateProgress(UpdateState.idle, refused: _refused);
    return UpdateProgress(
      _committed
          ? UpdateState.committed
          : _held.length == _length
          ? UpdateState.staged
          : UpdateState.receiving,
      refused: _refused,
      held: _held.length,
      version: _le32(h, 8),
      hash: h.sublist(16, 48),
    );
  });

  Never _refuse(Refusal r) {
    _refused = r;
    throw const AttException(AttError.refused);
  }

  @override
  Future<void> writeUpdate(Uint8List value) => _answer(() {
    if (value.isEmpty) throw const AttException(AttError.invalidLength);
    switch (value[0]) {
      case 0 when value.length == 1 + FirmwareImage.headerLen:
        final h = value.sublist(1);
        if (_committed) _refuse(Refusal.busy);
        _staging = null;
        _held.clear();
        if (String.fromCharCodes(h.sublist(0, 4)) != 'bark' || h[4] != 1) {
          _refuse(Refusal.header);
        }
        if (_le32(h, 8) <= image) _refuse(Refusal.version);
        // A node verifies each slot's Ed25519 signature; the fake takes a
        // header with any slot naming a key as signed.
        if (![for (var s = 0; s < 3; s++) h[48 + 65 * s]].any((k) => k != 0)) {
          _refuse(Refusal.signature);
        }
        _staging = h;
        _refused = null;
      case 1 when value.length == 1:
        if (_staging == null || _committed) _refuse(Refusal.incomplete);
        if (_held.length != _length) _refuse(Refusal.incomplete);
        if (lowBattery) _refuse(Refusal.battery);
        if (hex(sha256.convert(_held).bytes) !=
            hex(_staging!.sublist(16, 48))) {
          _staging = null;
          _held.clear();
          _refuse(Refusal.hash);
        }
        _committed = true;
        _refused = null;
      case 2 when value.length == 1:
        if (_committed && lowBattery) _refuse(Refusal.battery);
        _reboot();
        if (rebootAnswerLost) throw const AttException(0x85);
      case 0 || 1 || 2:
        throw const AttException(AttError.invalidLength);
      default:
        throw const AttException(AttError.valueNotAllowed);
    }
  });

  @override
  Future<void> writePiece(Uint8List value) => _answer(() {
    if (value.length <= pieceHeader) {
      throw const AttException(AttError.invalidLength);
    }
    _pieces++;
    if (_pieces == closeAfterPieces) scheduleMicrotask(close);
    final drop = dropEvery;
    if (drop != null && _pieces % drop == 0) return;
    final at = (value[0] << 24) | (value[1] << 16) | (value[2] << 8) | value[3];
    final bytes = value.sublist(pieceHeader);
    if (_staging == null ||
        _committed ||
        at != _held.length ||
        at + bytes.length > _length) {
      return;
    }
    _held.addAll(bytes);
  });

  @override
  Future<void> hurry() => _answer(() {});

  /// Ends the link and resets; a committed image runs once the swap is
  /// over, until then the fake takes no connection.
  void _reboot() {
    final committed = _committed;
    final version = committed ? _le32(_staging!, 8) : image;
    _staging = null;
    _held.clear();
    _committed = false;
    _refused = null;
    _backAt = now().add(committed ? swapTime : Duration.zero);
    image = version;
    if (committed) {
      // A boot: its first rotation again, and the relays learnt anew.
      _boot = _backAt!;
      _relays = 0;
      _heard = 0;
      for (final t in _learning) {
        t.cancel();
      }
      if (hearsAfterUpdate) {
        _hearNeighbours(swapTime, sleepy: onlySleepyAfterUpdate);
      }
    }
    scheduleMicrotask(close);
  }

  @override
  Future<void> setRecipient(int tag, [String? name]) => _answer(() {
    final v = encodeRecipient(tag, name);
    if (v.length > 1 && version < phonesVersion) {
      throw const AttException(AttError.invalidLength);
    }
    final r = _takeRecipient(v, this);
    ownerTag = r.tag;
    ownerName = r.name;
  });

  /// Recipient as the node takes it from `phone`: one that, by the
  /// versions it writes, it and the node do not serve each other is
  /// answered 0x82 and its link ended (spec "Phone link", Versions).
  ({int tag, int version, int oldest, String name}) _takeRecipient(
    Uint8List v,
    NodeLink phone,
  ) {
    final r = decodeRecipient(v);
    if (version >= phonesVersion &&
        (r.version < oldest || version < r.oldest)) {
      scheduleMicrotask(phone.close);
      throw const AttException(AttError.incompatible);
    }
    _app(phone, r.version);
    _notify();
    return r;
  }

  /// The oldest link version whose apps it serves, from link version 13.
  int oldest = 12;

  /// Node as a phone reads it: `room` from link version 13.
  NodeInfo get _info => NodeInfo(
    version,
    nodeId,
    image,
    room: version >= phonesVersion ? room : null,
    oldest: version >= phonesVersion ? oldest : null,
  );

  /// The records of the node's other phones, each another person's phone,
  /// a home one left behind, or both, which a test sets (spec "Phone
  /// link", Pairing, Phones). This phone's own is made of [paired],
  /// [homeKey], [ownerTag] and [ownerName], numbered [ownNumber].
  final List<PhoneRecord> others = [];

  /// The number of this phone's record.
  int ownNumber = 1;

  /// The name this phone's person wrote to Recipient.
  String ownerName = '';

  /// Today as a record counts it ([dayOf]).
  int get _today => now().toUtc().difference(DateTime.utc(2026)).inDays + 1;

  /// The records as Phones lists them, this phone's first while it holds
  /// a bond or a home.
  List<PhoneRecord> get records => [
    if (paired || homeKey != null)
      PhoneRecord(
        number: ownNumber,
        paired: paired,
        home: homeKey != null,
        linked: _owner.connected,
        mine: true,
        tag: ownerTag,
        day: _today,
        name: ownerName,
      ),
    ...others,
  ];

  /// How many more phones may pair: the records free, and 1 on a node
  /// nobody is paired with whose every record holds a home left behind.
  int get room {
    final rs = records;
    final free = maxPhones - rs.length;
    return free == 0 && !rs.any((r) => r.paired) ? 1 : free;
  }

  /// Whether a pairing from this phone is taken: its record holds its
  /// bond, or there is room.
  bool get takesPairing => paired || room > 0;

  int _phonesFrom = 0;

  /// The removal code on the screen, and when it was drawn.
  int? shownCode;
  DateTime? _codeAt;
  final Random _codes = Random(168);

  /// The bytes one value holds at the MTU the node offers.
  static const _valueRoom = attMtu - 3;

  @override
  Future<void> setPhonesFrom(int slot) => _answer(() {
    encodePhonesFrom(slot);
    _phonesFrom = slot;
  });

  /// As many whole records from the place written as fit one value, as
  /// the node reads them.
  @override
  Future<PhonesPage> readPhones() => _answer(() {
    final rs = records;
    final out = <PhoneRecord>[];
    var len = 1;
    var next = 0;
    for (var i = _phonesFrom; i < rs.length; i++) {
      final need = rs[i].encode().length;
      if (len + need > _valueRoom) {
        next = i;
        break;
      }
      out.add(rs[i]);
      len += need;
    }
    return PhonesPage(next, out);
  });

  @override
  Future<void> askRemovalCode() => _answer(() {
    shownCode = _codes.nextInt(removalCodes);
    _codeAt = now();
  });

  /// Takes a removal as the node does: a record or a part it does not
  /// hold is 0x13 and leaves the code untried; another code, or one past
  /// its 30 s, is 0x81; either way a code is tried once.
  @override
  Future<void> removePhone(
    int number, {
    required bool bond,
    required bool home,
    required int code,
  }) => _answer(() {
    encodeRemove(number, bond: bond, home: home, code: code);
    final r = records.where((r) => r.number == number).firstOrNull;
    if (r == null || (bond && !r.paired) || (home && !r.home)) {
      throw const AttException(AttError.valueNotAllowed);
    }
    final at = _codeAt;
    final taken =
        shownCode == code &&
        at != null &&
        now().difference(at) <= removalCodeTime;
    shownCode = null;
    _codeAt = null;
    if (!taken) throw const AttException(AttError.refused);
    if (r.mine) {
      if (home) homeKey = null;
      if (bond) {
        paired = false;
        ownerTag = null;
        scheduleMicrotask(close);
      }
      return;
    }
    final i = others.indexOf(r);
    final left = PhoneRecord(
      number: r.number,
      paired: r.paired && !bond,
      home: r.home && !home,
      tag: r.tag,
      day: r.day,
      name: r.name,
    );
    if (left.paired || left.home) {
      others[i] = left;
    } else {
      others.removeAt(i);
    }
  });

  @override
  Future<bool> readPublic() => _answer(() => public);

  @override
  Future<void> setPublic(bool v) => _answer(() => public = v);

  /// Forgets this phone's bond, as the node does: public again, its
  /// owner's tag gone, and this phone's next link refused, as the phone
  /// still holds its half of the bond; the link ends after the answer.
  @override
  Future<void> unpair() => _answer(() {
    public = true;
    ownerTag = null;
    paired = false;
    scheduleMicrotask(close);
  });

  @override
  Future<void> subscribeAsk() => _answer(() {});

  /// Sends the question only to a neighbour in its table, as the node
  /// does; the answer is the owner's, notified if its link is still up.
  @override
  Future<void> ask(List<int> id) => _answer(() {
    if (id.length != nodeIdLen) {
      throw const AttException(AttError.invalidLength);
    }
    final key = hex(id);
    asked.add(key);
    if (!neighbours.values.any((n) => hex(n.id) == key)) return;
    final s = statuses[key];
    if (s == null) return;
    _answerable++;
    if (loseEvery > 0 && _answerable % loseEvery == 0) return;
    _timers.add(
      Timer(answerAfter, () {
        if (_owner.connected) _owner.answer.add(Answer(id, s));
      }),
    );
  });

  @override
  Stream<Status> get statusNotified => _owner.status.stream;

  @override
  Stream<GivenBack?> get givenBackNotified => _owner.given.stream;

  @override
  Stream<Answer?> get answerNotified => _owner.answer.stream;

  /// Whether it serves phones that have not paired, as a node does
  /// unless its owner makes it private; kept across connections and
  /// pairings as with the bond.
  bool get public => _public;
  bool _public = true;
  set public(bool v) {
    _public = v;
    // Made private, it closes the guest's link.
    if (!v) {
      for (final g in [...guests]) {
        g.close();
      }
    }
    _turnGen++;
    _idleCheck();
    _notify();
  }

  /// The paired phone's recipient tag, kept with the bond; null until it
  /// writes one.
  int? ownerTag;

  /// Whether any phone has paired with it; a test's stranger's node has
  /// no owner, and all its mail is the guests'.
  bool owned = true;

  /// The guests' links up now: one, and for a moment a second, which
  /// the node closes once it writes Recipient.
  final List<FakeGuest> guests = [];

  /// The guests it serves at once beside its owner (spec "Phone link",
  /// Public).
  static const maxGuests = 1;

  /// How long it keeps a guest's link: the ATT transaction's timeout,
  /// 30 s (spec "Phone link", Public).
  static const guestLinkTime = Duration(seconds: 30);

  /// The guests' items of its ring, oldest first, as the node keeps them
  /// in flash: on a public node every message not the owner's.
  final List<GuestItem> ring = [];

  /// The items its ring holds, as a home's (spec "Mailbox", The ring);
  /// the oldest leaves for a new one.
  static const ringItems = 1024;

  /// The last number an item took; the owner's take numbers too, so the
  /// guests' run with gaps, as in the node's ring.
  int _number = 0;

  /// A message the network delivered for its phones, as a test makes one.
  void deliver(Uint8List payload) {
    _place(_message(payload));
    _notify();
  }

  /// Keeps a message for its phones: the owner's in the owner's inbox,
  /// those with the owner's tag or every one while the bond holds none;
  /// on a public node the rest in the ring, by number, for the guests to
  /// read by their tags, and all of them on a node nobody has paired with.
  void _place(InboxMessage m) {
    _number++;
    final tag = m.payload.length > 1 ? m.payload[1] : null;
    final owners = owned && (ownerTag == null || ownerTag == tag);
    if (!public || owners) {
      _inbox.add(m);
      return;
    }
    ring.add(GuestItem(_number, m));
    if (ring.length > ringItems) ring.removeAt(0);
  }

  /// The oldest item of the ring after `after` with the tag `tag`; a
  /// number above the newest is read as 0, as the ring has restarted
  /// since the guest wrote it.
  GuestItem? _guestItem(int? tag, int after) {
    final from = after > _number ? 0 : after;
    return ring
        .where((i) => i.number > from && i.message.payload[1] == tag)
        .firstOrNull;
  }

  /// Its ring starts again from nothing, numbers from 1, as a node's
  /// erased flash does.
  void restartRing() {
    ring.clear();
    _number = 0;
  }

  /// A phone that has not paired connects; on a node that is not public
  /// every value it touches is refused for want of the bond. Its link
  /// ends after [guestLinkTime].
  FakeGuest connectGuest() {
    final g = FakeGuest._(this);
    guests.add(g);
    _joined(g);
    _timers.add(Timer(guestLinkTime, g.close));
    return g;
  }

  /// A guest's link ended: its turn passes.
  void _left(FakeGuest g) {
    guests.remove(g);
    _gone(g);
  }

  /// The phone the app runs on holds the fake's bond; false once
  /// [pairElsewhere] has it pair with another (spec "Phone link").
  bool paired = true;

  /// Another phone removes this one's bond, or before link version 13
  /// pairs in its place, and the fake then refuses this one; the tag kept
  /// with the old bond goes with it.
  void pairElsewhere() {
    paired = false;
    ownerTag = null;
  }

  /// This phone pairs again, by the code on the fake's screen.
  void pair() => paired = true;

  /// The fake runs on while the app is away, as a node does, so a
  /// message can be given back or arrive while it is.
  NodeLink connect() {
    if (!paired) throw NotOurs(nodeName(nodeId));
    final back = _backAt;
    if (back != null && now().isBefore(back)) {
      throw StateError('${nodeName(nodeId)} is swapping in its new image');
    }
    if (!_owner.connected) _joined(this);
    _owner.connected = true;
    _closed = Completer<void>();
    return this;
  }

  @override
  Future<void> get closed => _closed.future;

  @override
  Future<void> close() async {
    if (_owner.connected) _gone(this);
    _owner.connected = false;
    if (!_closed.isCompleted) _closed.complete();
  }

  /// Switches the fake off.
  void dispose() {
    close();
    for (final g in [...guests]) {
      g.close();
    }
    for (final t in _timers) {
      t.cancel();
    }
  }
}

/// One phone's side of a link: what the node notifies it and has given
/// back to it.
class _Phone {
  bool connected = false;
  (int, DateTime)? givenBack;
  final status = StreamController<Status>.broadcast();
  final given = StreamController<GivenBack?>.broadcast();
  final answer = StreamController<Answer?>.broadcast();
}

/// A guest's link to a public [FakeNode]: Node, Status, Send, Inbox, Given
/// back and Recipient; every other value is answered insufficient
/// authentication, 0x05, as on a link without the bond.
class FakeGuest implements NodeLink {
  final FakeNode node;
  final _Phone _phone = _Phone()..connected = true;
  final _closed = Completer<void>();

  /// The number of the last item it took, as written to Guest; 0 at each
  /// connection.
  int _after = 0;

  /// The tag it wrote to Recipient.
  int? tag;

  FakeGuest._(this.node);

  Future<T> _open<T>(T Function() f) async {
    if (!_phone.connected) throw StateError('not connected');
    if (!node.public) _shut();
    return f();
  }

  Never _shut() =>
      throw const AttException(AttError.insufficientAuthentication);

  Future<T> _bonded<T>() async {
    if (!_phone.connected) throw StateError('not connected');
    _shut();
  }

  @override
  Future<NodeInfo> readNode() => _open(() => node._info);

  @override
  Future<void> subscribe() => _open(() {});

  @override
  Future<Status> readStatus() => _open(() => node._statusOf(this));

  @override
  Future<GivenBack?> readGivenBack() => _open(() => node._givenBackNow(_phone));

  /// A guest's Inbox gives the Guest form, [readGuest].
  @override
  Future<InboxMessage?> readInbox() async =>
      throw StateError('a guest\'s Inbox gives items in the Guest form');

  /// A guest's items stay in the ring: a write to Inbox is not allowed.
  @override
  Future<void> confirm(int id) =>
      _open(() => throw const AttException(AttError.valueNotAllowed));

  @override
  Future<void> send(Uint8List value) => _open(() => node._send(this, value));

  /// A second guest that writes Recipient while another's link is up is
  /// closed at once: the other link is the owner's to take.
  @override
  Future<void> setRecipient(int tag, [String? name]) => _open(() {
    node._takeRecipient(encodeRecipient(tag, name), this);
    if (node.guests.length > FakeNode.maxGuests) {
      close();
      throw StateError('closed: another guest has the link');
    }
    this.tag = tag;
  });

  @override
  Future<void> setGuestAfter(int number) => _open(() {
    encodeAfter(number);
    _after = number;
  });

  @override
  Future<GuestItem?> readGuest() => _open(() => node._guestItem(tag, _after));

  @override
  Future<void> setMapPoint(Position? p) => _bonded();
  @override
  Future<void> setClock(int utcS, int offsetMin) => _bonded();
  @override
  Future<NodeTime> readClock() => _bonded();
  @override
  Future<void> setHome(List<int>? key) => _bonded();
  @override
  Future<NodeHome> readHome() => _bonded();
  @override
  Future<void> setNeighboursFrom(int slot) => _bonded();
  @override
  Future<NeighbourPage> readNeighbours() => _bonded();
  @override
  Future<UpdateProgress> readUpdate() => _bonded();
  @override
  Future<void> writeUpdate(Uint8List value) => _bonded();
  @override
  Future<void> writePiece(Uint8List value) => _bonded();
  @override
  Future<bool> readPublic() => _bonded();
  @override
  Future<void> setPublic(bool public) => _bonded();
  @override
  Future<void> unpair() => _bonded();
  @override
  Future<void> subscribeAsk() => _bonded();
  @override
  Future<void> ask(List<int> id) => _bonded();
  @override
  Future<void> setPhonesFrom(int slot) => _bonded();
  @override
  Future<PhonesPage> readPhones() => _bonded();
  @override
  Future<void> askRemovalCode() => _bonded();
  @override
  Future<void> removePhone(
    int number, {
    required bool bond,
    required bool home,
    required int code,
  }) => _bonded();

  @override
  Future<void> hurry() => _open(() {});

  @override
  Stream<Status> get statusNotified => _phone.status.stream;

  @override
  Stream<GivenBack?> get givenBackNotified => _phone.given.stream;

  @override
  Stream<Answer?> get answerNotified => _phone.answer.stream;

  @override
  Future<void> get closed => _closed.future;

  @override
  Future<void> close() async {
    if (!_phone.connected) return;
    _phone.connected = false;
    node._left(this);
    if (!_closed.isCompleted) _closed.complete();
  }
}
