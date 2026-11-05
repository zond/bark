// A node in Dart that answers as the spec's "Phone link" says, so the
// app can be tried on a phone with no board. Its network is invented:
// it hears relays, gives some messages back and answers each message it
// takes with an echo in its inbox.

import 'dart:async';
import 'dart:typed_data';

import 'link.dart';
import 'node_link.dart';
import 'payload.dart';

class FakeNode implements NodeLink {
  final DateTime Function() now;

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

  /// The time the phone gave, against the fake's own clock, and the
  /// offset; the fake has no GPS time.
  (int, DateTime)? _time;
  int _offsetMin = 0;
  int _relays = 0;
  bool _ready = true;
  final List<InboxMessage> _inbox = [];
  final Map<int, Address> _senders = {};
  (int, DateTime)? _givenBack;
  int _writes = 0;
  int _taken = 0;
  bool _connected = false;
  final _status = StreamController<Status>.broadcast();
  final _given = StreamController<GivenBack?>.broadcast();
  var _closed = Completer<void>();
  final List<Timer> _timers = [];

  FakeNode({required this.now}) {
    _boot = now();
    final step = firstRotation ~/ (relaysHeard + 1);
    for (var i = 1; i <= relaysHeard; i++) {
      _after(step * i, () => _relays++);
    }
    _after(firstRotation, () {});
    _inbox.add(_from(_own, 'Hello from the fake node.'));
    _inbox.add(_from(_own, 'A second message, waiting in the inbox.'));
  }

  Address get _own => Address(_mapPoint ?? gps, id);

  InboxMessage _from(Address sender, String text) {
    final p = encodeText(text);
    final m = InboxMessage(msgId(_own, p), p);
    _senders[m.msgId] = sender;
    return m;
  }

  /// Who sent a message in its inbox: itself for the two it starts
  /// with, and for an echo the address the echoed message went to. The
  /// payload will carry this (spec "Phone link", "The payload"); the
  /// prototype's does not.
  Address? senderOf(InboxMessage m) => _senders[m.msgId];

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
      mapPoint: _mapPoint != null,
      placed: true,
      booting: left > Duration.zero,
      relays: _relays,
      bootLeftS: _ceilS(left),
      address: _mapPoint ?? gps,
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
      final text = decodeText(payload);
      _after(
        echoAfter,
        () => _inbox.add(_from(dst, 'Echo: ${text ?? hex(payload)}')),
      );
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
