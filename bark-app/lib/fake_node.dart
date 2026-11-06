// A node in Dart that answers as the spec's "Phone link" says, so the
// app can be tried on a phone with no board. Its network is invented:
// it hears relays, gives some messages back, and behind it is one
// person, the fake friend, who writes first and echoes every text sent
// to them, sealed as a real phone would. The fake friend has a home and
// writes from a node away from it, which moves between their messages.
// Somewhere away is the person's home node, whose mailbox answers a
// fetch as the spec's "Mailbox" says.

import 'dart:async';
import 'dart:math';
import 'dart:typed_data';

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

  /// The fake's GPS fix.
  static final gps = Position.fromDegrees(50.45, 30.52);

  /// A relay's first rotation (spec "Channel at boot").
  static const firstRotation = Duration(seconds: 195);

  /// Relays the fake hears, spread over its first rotation.
  static const relaysHeard = 3;

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
  final List<InboxMessage> _inbox = [];

  /// The fake friend's last `sent`, and how many messages they wrote.
  int _friendSent = 0;
  int _friendWrote = 0;
  (int, DateTime)? _givenBack;
  int _writes = 0;
  int _taken = 0;
  bool _connected = false;
  final _status = StreamController<Status>.broadcast();
  final _given = StreamController<GivenBack?>.broadcast();
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
    _inbox.add(_intro('Hello from the fake node.'));
    _inbox.add(_intro('A second message, waiting in the inbox.'));
  }

  Address get _own => Address(_mapPoint ?? _fix, id);

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
      writePlain(_header(), [
        CardRecord(friendCard.encode()),
        TextRecord(text),
      ]),
    ),
  );

  /// The text of a message from the app to the fake friend, if it is one.
  String? _textTo(Uint8List payload) {
    try {
      final plain = switch (formOf(payload)) {
        Form.boxed => openBox(_pair, payload),
        Form.introduction => openIntro(sodium, friend, payload),
        _ => null,
      };
      if (plain == null) return null;
      final (_, rs) = readPlain(plain);
      return rs.whereType<TextRecord>().firstOrNull?.text;
    } on BadPayload {
      return null;
    }
  }

  /// The home away's answer to a fetch, as its mailbox gives it: every
  /// item after `since`, or one reply that says nothing is new.
  void _answerFetch(Uint8List payload) {
    final key = mailbox;
    if (key == null || formOf(payload) != Form.fetch) return;
    final Fetch f;
    try {
      f = Fetch.open(sodium, key, payload);
    } on BadPayload {
      return;
    }
    final first = f.since + 1;
    final replies = [
      for (var n = first; n <= homeRing.length; n++)
        sealReply(sodium, key, f.sent, n - first + 1, n, homeRing[n - 1]),
    ];
    if (replies.isEmpty) replies.add(sealNothingNew(sodium, key, f.sent));
    _after(fetchAnswerAfter, () {
      for (final r in replies) {
        _inbox.add(_message(r));
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
    if (_connected) _status.add(_now());
  }

  Status _now() {
    final left = firstRotation - now().difference(_boot);
    return Status(
      ready: _ready,
      externalPower: true,
      sleepy: sleepy,
      carried: carried,
      mapPoint: _mapPoint != null,
      placed: true,
      booting: left > Duration.zero,
      relays: _relays,
      bootLeftS: _ceilS(left),
      address: _mapPoint ?? _fix,
      inbox: _inbox.length,
    );
  }

  static int _ceilS(Duration d) => d <= Duration.zero
      ? 0
      : (d.inMicroseconds + Duration.microsecondsPerSecond - 1) ~/
            Duration.microsecondsPerSecond;

  GivenBack? _givenBackNow() {
    final g = _givenBack;
    if (g == null) return null;
    return GivenBack(g.$1, _ceilS(g.$2.difference(now())));
  }

  Future<T> _answer<T>(T Function() f) async {
    if (!_connected) throw StateError('not connected');
    return f();
  }

  @override
  Future<NodeInfo> readNode() => _answer(() => NodeInfo(linkVersion, id, 1));

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
  Future<void> send(Uint8List value) => _answer(() {
    if (value.length < Address.len || value.length > Address.len + maxPayload) {
      throw const AttException(AttError.invalidLength);
    }
    _writes++;
    if (!_ready || _writes % notReadyEvery == 0) {
      // As when the bucket empties between the status the app read and
      // its write: not ready for one ban.
      if (_ready) {
        _ready = false;
        _notify();
        _after(ban, () => _ready = true);
      }
      throw const AttException(AttError.notReady);
    }
    final dst = Address(
      Position.decode(value),
      value.sublist(Position.wireLen, Address.len),
    );
    final payload = value.sublist(Address.len);
    final id = msgId(dst, payload);
    _taken++;
    _ready = false;
    if (_givenBack != null) {
      _givenBack = null;
      if (_connected) _given.add(null);
    }
    _notify();
    if (_taken % givenBackEvery == 0) {
      _after(handOver, () {
        _givenBack = (id, now().add(givenBackWait));
        if (_connected) _given.add(_givenBackNow());
      });
      _after(handOver + givenBackWait, () => _ready = true);
    } else {
      _after(handOver, () => _ready = true);
      _answerFetch(payload);
      final text = _textTo(payload);
      if (text != null) {
        _after(
          echoAfter,
          () => _inbox.add(
            _message(
              sealBox(
                _pair,
                sodium.randombytes.buf(nonceLen),
                writePlain(_header(), [TextRecord('Echo: $text')]),
              ),
            ),
          ),
        );
      }
    }
  });

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
  Stream<Status> get statusNotified => _status.stream;

  @override
  Stream<GivenBack?> get givenBackNotified => _given.stream;

  /// The fake runs on while the app is away, as a node does, so a
  /// message can be given back or arrive while it is.
  NodeLink connect() {
    _connected = true;
    _closed = Completer<void>();
    return this;
  }

  @override
  Future<void> get closed => _closed.future;

  @override
  Future<void> close() async {
    _connected = false;
    if (!_closed.isCompleted) _closed.complete();
  }

  /// Switches the fake off.
  void dispose() {
    close();
    for (final t in _timers) {
      t.cancel();
    }
  }
}
