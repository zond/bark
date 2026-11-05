// The app's side of the phone link (spec "Phone link"): the phone is the
// queue, it drains the node's inbox, and it matches what the node gives
// back to the messages it sent.

import 'dart:async';

import 'package:flutter/foundation.dart';

import 'link.dart';
import 'node_link.dart';
import 'payload.dart';
import 'store.dart';

enum OutState {
  /// Waiting in the app for the node to be ready.
  queued,

  /// Written; the node has not answered yet.
  writing,

  /// The node has it.
  taken,

  /// The node gave it back; it may be offered again once the wait is over.
  givenBack,

  /// The node will never take it as written: it answered the write with
  /// an error other than not ready.
  refused,
}

class Outgoing {
  final Address dst;
  final Uint8List payload;
  final int id;

  /// When the person queued it, for ordering a conversation.
  final DateTime at;
  OutState state;
  DateTime? retryAt;
  String? error;

  Outgoing(
    this.dst,
    this.payload, {
    required this.at,
    this.state = OutState.queued,
  }) : id = msgId(dst, payload);

  String get text => decodeText(payload) ?? hex(payload);

  Map<String, dynamic> toJson() => {
    'dst': dst.toString(),
    'payload': hex(payload),
    'at': at.toIso8601String(),
    // An answer the app did not hear may have been a take; offered again,
    // the same message has the same ID and is one message to every node.
    'state': (state == OutState.writing ? OutState.queued : state).name,
    'retryAt': retryAt?.toIso8601String(),
    'error': error,
  };

  static Outgoing fromJson(Map<String, dynamic> j) =>
      Outgoing(
          Address.parse(j['dst'] as String)!,
          parseHex(j['payload'] as String)!,
          // The app's first version did not keep it; its messages sort
          // first.
          at: j['at'] == null
              ? DateTime.fromMillisecondsSinceEpoch(0)
              : DateTime.parse(j['at'] as String),
          state: OutState.values.byName(j['state'] as String),
        )
        ..retryAt = j['retryAt'] == null
            ? null
            : DateTime.parse(j['retryAt'] as String)
        ..error = j['error'] as String?;
}

class Received {
  final int id;
  final Uint8List payload;
  final DateTime at;

  /// The sender's address, or null when the app cannot tell (see
  /// [Session.senderOf]).
  final Address? from;
  Received(this.id, List<int> payload, this.at, {this.from})
    : payload = Uint8List.fromList(payload);

  /// The text, or the payload in hex if it is not in the prototype's
  /// format.
  String get text => decodeText(payload) ?? '(not text) ${hex(payload)}';

  Map<String, dynamic> toJson() => {
    'id': id,
    'payload': hex(payload),
    'at': at.toIso8601String(),
    'from': from?.toString(),
  };

  static Received fromJson(Map<String, dynamic> j) => Received(
    j['id'] as int,
    parseHex(j['payload'] as String)!,
    DateTime.parse(j['at'] as String),
    from: j['from'] == null ? null : Address.parse(j['from'] as String),
  );
}

/// A line of the link's event log.
typedef LogLine = ({DateTime at, String text});

Address? _senderUnknown(InboxMessage m) => null;

class Session extends ChangeNotifier {
  final Store store;
  final DateTime Function() now;

  Session({required this.store, required this.now});

  /// Who sent a message in the inbox. The sender's address is to travel
  /// inside the payload (spec "Phone link", "The payload"), which the
  /// prototype's does not carry, so a real node's messages have no known
  /// sender; the fake node knows whom it echoes.
  Address? Function(InboxMessage) senderOf = _senderUnknown;

  /// The last value of Given back read or notified.
  GivenBack? givenBack;

  /// The link's events since the app started, newest last; the oldest
  /// go once there are [logLines], which is more than a bench session
  /// scrolls back through.
  final List<LogLine> log = [];
  static const logLines = 500;

  NodeLink? _link;
  NodeInfo? node;
  Status? status;
  DateTime? _statusAt;

  /// The last thing worth telling the person.
  String? notice;

  final List<Outgoing> outbox = [];
  final List<Received> received = [];

  bool _writing = false;
  bool _draining = false;
  final List<StreamSubscription<dynamic>> _subs = [];

  bool get connected => _link != null;

  /// The node's address as a person gives it to others: the position of
  /// its address and its ID, once it has a place.
  Address? get address {
    final s = status;
    final n = node;
    if (s == null || n == null || !s.placed) return null;
    return Address(s.address, n.id);
  }

  /// Seconds left of the first rotation, counted down here: the node
  /// does not notify the countdown.
  int get bootLeftS {
    final s = status;
    if (s == null || _statusAt == null) return 0;
    final left = s.bootLeftS - now().difference(_statusAt!).inSeconds;
    return left < 0 ? 0 : left;
  }

  int waitLeftS(Outgoing o) {
    final at = o.retryAt;
    if (at == null) return 0;
    final left = at.difference(now());
    return left.isNegative ? 0 : (left.inMilliseconds + 999) ~/ 1000;
  }

  Future<void> load() async {
    final j = await store.load();
    if (j == null) return;
    outbox.addAll(
      (j['outbox'] as List).map(
        (e) => Outgoing.fromJson(e as Map<String, dynamic>),
      ),
    );
    received.addAll(
      (j['received'] as List).map(
        (e) => Received.fromJson(e as Map<String, dynamic>),
      ),
    );
    notifyListeners();
  }

  Future<void> _save() => store.save({
    'outbox': [for (final o in outbox) o.toJson()],
    'received': [for (final r in received) r.toJson()],
  });

  void _badValue(Object e) =>
      _tell('The node sent a value this app cannot read: $e');

  void _tell(String s) {
    notice = s;
    note(s);
    notifyListeners();
  }

  /// Adds a line to the log.
  void note(String s) {
    log.add((at: now(), text: s));
    if (log.length > logLines) log.removeAt(0);
    notifyListeners();
  }

  /// Takes a connected link through the checklist of the phone link's
  /// note: Node first, refused if newer than the app; then the
  /// subscriptions, Status and Given back, the inbox, the queue.
  Future<bool> attach(NodeLink link) async {
    final NodeInfo n;
    try {
      n = await link.readNode();
    } catch (_) {
      await link.close();
      rethrow;
    }
    if (n.version > linkVersion) {
      await link.close();
      _tell(
        'This node speaks link version ${n.version}, this app $linkVersion: '
        'the node\'s firmware is newer. Update the app.',
      );
      return false;
    }
    node = n;
    _link = link;
    notice = null;
    note(
      'Node: ${nodeName(n.id)}, link version ${n.version}, image ${n.image}',
    );
    _subs
      ..add(link.statusNotified.listen(_onStatus, onError: _badValue))
      ..add(link.givenBackNotified.listen(_onGivenBack, onError: _badValue));
    unawaited(link.closed.then((_) => _detach(link)));
    try {
      await link.subscribe();
      _onStatus(await link.readStatus());
      _onGivenBack(await link.readGivenBack());
    } catch (e) {
      _tell('Reading the node failed: $e');
    }
    return true;
  }

  Future<void> disconnect() async => _link?.close();

  void _detach(NodeLink link) {
    if (_link != link) return;
    for (final s in _subs) {
      s.cancel();
    }
    _subs.clear();
    _link = null;
    status = null;
    node = null;
    givenBack = null;
    note('Disconnected');
  }

  void _onStatus(Status s) {
    note(
      'Status: ready ${s.ready}, inbox ${s.inbox}, relays ${s.relays}, '
      'boot left ${s.bootLeftS} s',
    );
    status = s;
    _statusAt = now();
    notifyListeners();
    if (s.inbox > 0) _drain();
    _pump();
  }

  void _onGivenBack(GivenBack? g) {
    givenBack = g;
    note(
      g == null
          ? 'Given back: empty'
          : 'Given back: ${msgIdHex(g.msgId)}, wait ${g.waitS} s',
    );
    if (g == null) return;
    final o = outbox.where((o) => o.id == g.msgId).firstOrNull;
    if (o == null) {
      _tell(
        'The node gave back message ${msgIdHex(g.msgId)}, '
        'which this app did not send.',
      );
      return;
    }
    // The value stays until the node takes another message, so it is
    // read again at every connect; only a message the node has means it.
    if (o.state != OutState.taken && o.state != OutState.givenBack) return;
    o.state = OutState.givenBack;
    o.retryAt = now().add(Duration(seconds: g.waitS));
    notifyListeners();
    _save();
  }

  /// Queues a text to `dst`.
  Future<void> queue(Address dst, String text) async {
    outbox.add(Outgoing(dst, encodeText(text), at: now()));
    notifyListeners();
    await _save();
    _pump();
  }

  /// Offers a given-back message again; the node takes it when ready.
  Future<void> offerAgain(Outgoing o) async {
    o.state = OutState.queued;
    o.retryAt = null;
    notifyListeners();
    await _save();
    _pump();
  }

  Future<void> drop(Outgoing o) async {
    if (o.state == OutState.writing) return;
    outbox.remove(o);
    notifyListeners();
    await _save();
  }

  Future<void> _pump() async {
    final link = _link;
    if (_writing || link == null || !(status?.ready ?? false)) return;
    final o = outbox.where((o) => o.state == OutState.queued).firstOrNull;
    if (o == null) return;
    _writing = true;
    o.state = OutState.writing;
    notifyListeners();
    var again = true;
    try {
      await link.send(encodeSend(o.dst, o.payload));
      o.state = OutState.taken;
      note('Send ${msgIdHex(o.id)}: taken');
    } on AttException catch (e) {
      note('Send ${msgIdHex(o.id)}: $e');
      if (e.code == AttError.notReady) {
        // Ready turned false between the status read and the write; wait
        // for the node to say ready again, and read it in case that
        // notification came before this answer.
        o.state = OutState.queued;
        status = status?.copyWith(ready: false);
        again = false;
        unawaited(link.readStatus().then(_onStatus, onError: (_) {}));
      } else {
        o.state = OutState.refused;
        o.error = '$e';
      }
    } catch (e) {
      o.state = OutState.queued;
      again = false;
      _tell('Sending failed: $e');
    } finally {
      _writing = false;
    }
    notifyListeners();
    await _save();
    if (again) _pump();
  }

  Future<void> _drain() async {
    if (_draining) return;
    _draining = true;
    try {
      while (true) {
        final link = _link;
        if (link == null) break;
        final m = await link.readInbox();
        if (m == null) break;
        note('Inbox: ${msgIdHex(m.msgId)}, ${m.payload.length} bytes');
        await _keep(m);
        try {
          await link.confirm(m.msgId);
          note('Inbox: confirmed ${msgIdHex(m.msgId)}');
        } on AttException catch (e) {
          // The oldest is the one just read, so the node should not
          // refuse its ID; stop rather than read it in a loop.
          _tell('The node refused the inbox confirmation: $e');
          break;
        }
      }
    } catch (e) {
      _tell('Reading the inbox failed: $e');
    } finally {
      _draining = false;
      notifyListeners();
    }
  }

  /// Stores a message before it is confirmed. One read again, after a
  /// confirmation that was lost, is not kept twice.
  Future<void> _keep(InboxMessage m) async {
    final dup = received.any(
      (r) => r.id == m.msgId && listEquals(r.payload, m.payload),
    );
    if (dup) return;
    received.add(Received(m.msgId, m.payload, now(), from: senderOf(m)));
    notifyListeners();
    await _save();
  }

  Future<void> setMapPoint(Position? p) async {
    final link = _link;
    if (link == null) return;
    try {
      await link.setMapPoint(p);
      note('Map point: ${p ?? 'cleared'}');
    } catch (e) {
      _tell('Setting the map point failed: $e');
    }
  }

  @override
  void dispose() {
    for (final s in _subs) {
      s.cancel();
    }
    super.dispose();
  }
}
