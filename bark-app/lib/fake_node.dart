// A node in Dart that answers as the spec's "Phone link" says, so the
// app can be tried on a phone with no board. Its network is invented:
// it hears relays, gives some messages back, and behind it is one
// person, the fake friend, who writes first and echoes every text sent
// to them, sealed as a real phone would. The fake friend has a home and
// writes from a node away from it, which moves between their messages.
// Somewhere away is the person's home node, whose mailbox answers a
// fetch as the spec's "Mailbox" says. Around it stand a few neighbours,
// one of each link quality and of each kind a map draws apart. It takes
// a firmware update as the node does, and comes back from its reboot
// running the new version. Made public, it also takes guests' links, as
// the spec's "Phone link", Public, says.

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

  late final DateTime _boot;
  Position? _mapPoint;

  /// A carried node's address is its live fix, which wanders; a sleepy
  /// node's is not its stored place either.
  bool carried = false;
  bool sleepy = false;
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
    final step = firstRotation ~/ (relaysHeard + 1);
    for (var i = 1; i <= relaysHeard; i++) {
      _after(step * i, () => _relays++);
    }
    _after(firstRotation, () {});
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

  /// Runs `f` after `d`, then notifies the status.
  void _after(Duration d, void Function() f) {
    _timers.add(
      Timer(d, () {
        f();
        _notify();
      }),
    );
  }

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
      placed: true,
      booting: left > Duration.zero,
      relays: _relays,
      bootLeftS: _ceilS(left),
      address: _mapPoint ?? _fix,
      inbox: g == null ? _inbox.length : min(1, g._inbox.length),
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
  Future<NodeInfo> readNode() =>
      _answer(() => NodeInfo(linkVersion, nodeId, image));

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
    // On a public node the turn is Ready: a phone whose turn it is not
    // is answered not ready, as when the node takes none.
    if (public && _turn != p) throw const AttException(AttError.notReady);
    _turnGen++;
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
        _notify();
        _freeAfter(ban);
      }
      throw const AttException(AttError.notReady);
    }
    _taken++;
    _ready = false;
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
  /// set; null with no phone connected.
  Object? _turn;

  /// Counts the turn's changes and its holder's writes, so that a check a
  /// ban later sees whether the holder sat on it.
  int _turnGen = 0;

  /// Gives the turn to the next phone connected after the one that has
  /// it, in the order of their links, and starts its ban.
  void _passTurn() {
    final at = _turn == null ? -1 : _links.indexOf(_turn!);
    _turn = _links.isEmpty ? null : _links[(at + 1) % _links.length];
    _turnGen++;
    _idleCheck();
  }

  /// A phone that has had Ready for a ban without writing passes the
  /// turn on, so that one with nothing to send holds nobody up; alone, it
  /// would pass it to itself.
  void _idleCheck() {
    if (!public || !_ready || _turn == null || _links.length < 2) return;
    final gen = _turnGen;
    _after(ban, () {
      if (_ready && _turnGen == gen) _passTurn();
    });
  }

  void _joined(Object p) {
    _links.add(p);
    _turn ??= p;
    _turnGen++;
    _idleCheck();
  }

  void _gone(Object p) {
    final had = _turn == p;
    final at = _links.indexOf(p);
    _links.remove(p);
    if (!had) return;
    // The next in order now stands where the one gone stood.
    _turn = _links.isEmpty ? null : _links[at % _links.length];
    _turnGen++;
    _idleCheck();
  }

  /// Takes messages again after `d`; Ready comes back for the phone
  /// whose turn it is.
  void _freeAfter(Duration d) {
    _after(d, () {
      _ready = true;
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
  Future<NodeHome> readHome() => _answer(() => NodeHome(homeKey != null, 0, 0));

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

  /// The update: the header taken, the bytes held, and whether committed.
  Uint8List? _staging;
  final List<int> _held = [];
  bool _committed = false;
  Refusal? _refused;
  DateTime? _backAt;

  static int _le32(List<int> b, int at) =>
      b[at] | (b[at + 1] << 8) | (b[at + 2] << 16) | (b[at + 3] << 24);

  int get _length => _le32(_staging!, 12);

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
    scheduleMicrotask(close);
  }

  @override
  Future<void> setRecipient(int tag) => _answer(() {
    encodeRecipient(tag);
    ownerTag = tag;
  });

  @override
  Future<bool> readPublic() => _answer(() => public);

  @override
  Future<void> setPublic(bool v) => _answer(() => public = v);

  @override
  Stream<Status> get statusNotified => _owner.status.stream;

  @override
  Stream<GivenBack?> get givenBackNotified => _owner.given.stream;

  /// Whether it serves phones that have not paired, kept across
  /// connections and pairings as with the bond.
  bool get public => _public;
  bool _public = false;
  set public(bool v) {
    _public = v;
    _turnGen++;
    _idleCheck();
    _notify();
  }

  /// The paired phone's recipient tag, kept with the bond; null until it
  /// writes one.
  int? ownerTag;

  /// The guests' links up now.
  final List<FakeGuest> guests = [];

  /// The guests it takes at once beside its owner, as the firmware plans;
  /// the app does not count on the number.
  static const maxGuests = 3;

  /// Messages it took for its phones and could place with none.
  int dropped = 0;

  /// A message the network delivered for its phones, as a test makes one.
  void deliver(Uint8List payload) {
    _place(_message(payload));
    _notify();
  }

  /// Hands a message to its phones: on a public node to every guest whose
  /// tag it carries, and to the owner when it is the owner's tag or the
  /// node holds none; what it can hand to nobody it drops.
  void _place(InboxMessage m) {
    if (!public) {
      _inbox.add(m);
      return;
    }
    final tag = m.payload.length > 1 ? m.payload[1] : null;
    var placed = false;
    for (final g in guests.where((g) => g.tag != null && g.tag == tag)) {
      g._inbox.add(m);
      placed = true;
    }
    if (ownerTag == null || ownerTag == tag) {
      _inbox.add(m);
      placed = true;
    }
    if (!placed) dropped++;
  }

  /// A phone that has not paired connects; on a node that is not public
  /// every value it touches is refused for want of the bond.
  FakeGuest connectGuest() {
    if (guests.length >= maxGuests) throw StateError('no link free');
    final g = FakeGuest._(this);
    guests.add(g);
    _joined(g);
    return g;
  }

  /// A guest's link ended: what waited for it goes, and its turn passes.
  void _left(FakeGuest g) {
    guests.remove(g);
    _gone(g);
    dropped += g._inbox.length;
    g._inbox.clear();
  }

  /// The phone the app runs on holds the fake's bond; false once
  /// [pairElsewhere] has it pair with another (spec "Phone link").
  bool paired = true;

  /// Another phone pairs with the fake, which then refuses this one; the
  /// tag kept with the old bond goes with it.
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
}

/// A guest's link to a public [FakeNode]: Node, Status, Send, Inbox, Given
/// back and Recipient; every other value is answered insufficient
/// authentication, 0x05, as on a link without the bond.
class FakeGuest implements NodeLink {
  final FakeNode node;
  final _Phone _phone = _Phone()..connected = true;
  final _closed = Completer<void>();

  /// What waits for this guest; it reads one at a time.
  final List<InboxMessage> _inbox = [];

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
  Future<NodeInfo> readNode() =>
      _open(() => NodeInfo(linkVersion, node.nodeId, node.image));

  @override
  Future<void> subscribe() => _open(() {});

  @override
  Future<Status> readStatus() => _open(() => node._statusOf(this));

  @override
  Future<GivenBack?> readGivenBack() => _open(() => node._givenBackNow(_phone));

  @override
  Future<InboxMessage?> readInbox() =>
      _open(() => _inbox.isEmpty ? null : _inbox.first);

  @override
  Future<void> confirm(int id) => _open(() {
    if (_inbox.isEmpty || _inbox.first.msgId != id) {
      throw const AttException(AttError.valueNotAllowed);
    }
    _inbox.removeAt(0);
    node._notify();
  });

  @override
  Future<void> send(Uint8List value) => _open(() => node._send(this, value));

  @override
  Future<void> setRecipient(int tag) => _open(() {
    encodeRecipient(tag);
    this.tag = tag;
  });

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
  Future<void> hurry() => _open(() {});

  @override
  Stream<Status> get statusNotified => _phone.status.stream;

  @override
  Stream<GivenBack?> get givenBackNotified => _phone.given.stream;

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
