// The app's side of the phone link (spec "Phone link"): the phone is the
// queue, it drains the node's inbox, and it matches what the node gives
// back to the messages it sent.

import 'dart:async';

import 'package:flutter/foundation.dart';

import 'link.dart';
import 'node_link.dart';
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

/// What a sent message carries for the person.
enum OutKind {
  /// A text.
  text,

  /// An acknowledgement of a received text.
  ack,

  /// The person's card.
  card,

  /// A fetch from the person's mailbox.
  fetch,
}

/// Which of a contact's addresses a message went to.
enum Toward {
  /// Where they last wrote from.
  there,

  /// Their home's mailbox, the address on their card.
  home,
}

class Outgoing {
  final Address dst;
  final Uint8List payload;
  final int id;

  /// When the person queued it, for ordering a conversation.
  final DateTime at;
  final OutKind kind;

  /// The contact's identity in hex; null for a fetch.
  final String? to;

  /// Its `sent`, by which the other side names it.
  final int sent;

  /// A text's words.
  final String? body;

  /// Which of the contact's addresses it went to; null for a fetch.
  final Toward? toward;

  /// The other side acknowledged it; a fetch, the home node answered.
  bool acked;
  OutState state;
  DateTime? retryAt;
  String? error;

  Outgoing(
    this.dst,
    this.payload, {
    required this.at,
    required this.kind,
    required this.sent,
    this.to,
    this.body,
    this.toward,
    this.acked = false,
    this.state = OutState.queued,
  }) : id = msgId(dst, payload);

  String get text => switch (kind) {
    OutKind.text => body ?? '',
    OutKind.ack => 'Acknowledged',
    OutKind.card => 'Your card',
    OutKind.fetch => 'Fetch from your mailbox',
  };

  Map<String, dynamic> toJson() => {
    'dst': dst.toString(),
    'payload': hex(payload),
    'at': at.toIso8601String(),
    'kind': kind.name,
    'to': to,
    'sent': sent,
    'body': body,
    'toward': toward?.name,
    'acked': acked,
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
          at: DateTime.parse(j['at'] as String),
          kind: OutKind.values.byName(j['kind'] as String),
          to: j['to'] as String?,
          sent: j['sent'] as int,
          body: j['body'] as String?,
          toward: j['toward'] == null
              ? null
              : Toward.values.byName(j['toward'] as String),
          acked: j['acked'] as bool,
          state: OutState.values.byName(j['state'] as String),
        )
        ..retryAt = j['retryAt'] == null
            ? null
            : DateTime.parse(j['retryAt'] as String)
        ..error = j['error'] as String?;
}

/// A received text, or a payload that did not open: kept for the debug
/// screen, in no conversation.
class Received {
  final int id;
  final Uint8List payload;
  final DateTime at;

  /// The sender's identity in hex; null if the payload did not open.
  final String? from;

  /// Its `sent`, by which an acknowledgement names it.
  final int? sent;

  /// The text.
  final String? body;

  /// It came out of the person's mailbox.
  final bool mailbox;

  /// The person acknowledged it.
  bool acked;

  Received(
    this.id,
    List<int> payload,
    this.at, {
    this.from,
    this.sent,
    this.body,
    this.mailbox = false,
    this.acked = false,
  }) : payload = Uint8List.fromList(payload);

  String get text => body ?? '(did not open) ${hex(payload)}';

  Map<String, dynamic> toJson() => {
    'id': id,
    'payload': hex(payload),
    'at': at.toIso8601String(),
    'from': from,
    'sent': sent,
    'body': body,
    'mailbox': mailbox,
    'acked': acked,
  };

  static Received fromJson(Map<String, dynamic> j) => Received(
    j['id'] as int,
    parseHex(j['payload'] as String)!,
    DateTime.parse(j['at'] as String),
    from: j['from'] as String?,
    sent: j['sent'] as int?,
    body: j['body'] as String?,
    mailbox: j['mailbox'] as bool,
    acked: j['acked'] as bool,
  );
}

/// A line of the link's event log.
typedef LogLine = ({DateTime at, String text});

/// A log line's local time of day, to the millisecond.
String logTime(DateTime at) => at.toLocal().toIso8601String().substring(11, 23);

class Session extends ChangeNotifier {
  final Store store;
  final DateTime Function() now;

  Session({required this.store, required this.now});

  /// Reads a message from the inbox and keeps what it says; without one
  /// the payload is kept as it came.
  Future<void> Function(InboxMessage)? reader;

  /// Told of each message kept, once.
  void Function(Received)? kept;

  /// The mailbox key a node should hold, if it is the person's home.
  Uint8List? Function(NodeInfo)? homeKey;

  /// The last value of Given back read or notified.
  GivenBack? givenBack;

  /// The link's events since the app started, newest last; the oldest
  /// go once there are [logLines], which is more than a bench session
  /// scrolls back through.
  final List<LogLine> log = [];
  static const logLines = 500;

  /// Told of each log line as it is added.
  void Function(LogLine)? noted;

  NodeLink? _link;
  NodeInfo? node;
  Status? status;

  /// The node's time as last read, after the app wrote its own.
  NodeTime? nodeTime;

  /// The node's Home as last read, from link version 3.
  NodeHome? nodeHome;
  DateTime? _statusAt;

  /// The last thing worth telling the person.
  String? notice;

  final List<Outgoing> outbox = [];
  final List<Received> received = [];

  /// The node this phone is paired with, by name; null before the first.
  String? pairedWith;

  /// That node no longer holds this phone's bond: another phone has
  /// paired with it since (spec "Phone link", the replaced phone).
  bool pairingLost = false;

  /// The person closed the link or stopped looking for the paired node:
  /// the app does not look for it until they connect. Kept, so that the
  /// app started with the phone does not look either.
  bool stopped = false;

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

  /// The version of `bark.json`'s format.
  static const version = 1;

  Future<void> load() async {
    final j = await store.load(
      version,
      (j) => (
        outbox: [
          for (final e in j['outbox'] as List)
            Outgoing.fromJson(e as Map<String, dynamic>),
        ],
        received: [
          for (final e in j['received'] as List)
            Received.fromJson(e as Map<String, dynamic>),
        ],
        // Added after the format's first version: absent means none.
        pairedWith: j['pairedWith'] as String?,
        pairingLost: j['pairingLost'] as bool? ?? false,
        stopped: j['stopped'] as bool? ?? false,
      ),
    );
    if (j == null) return;
    outbox.addAll(j.outbox);
    received.addAll(j.received);
    pairedWith = j.pairedWith;
    pairingLost = j.pairingLost;
    stopped = j.stopped;
    notifyListeners();
  }

  Future<void> save() => store.save(version, {
    'outbox': [for (final o in outbox) o.toJson()],
    'received': [for (final r in received) r.toJson()],
    'pairedWith': pairedWith,
    'pairingLost': pairingLost,
    'stopped': stopped,
  });

  /// The node `name` holds this phone's bond, or no longer does.
  Future<void> setPaired(String name, {bool lost = false}) async {
    if (pairedWith == name && pairingLost == lost) return;
    pairedWith = name;
    pairingLost = lost;
    note(lost ? 'Pairing: $name is no longer ours' : 'Pairing: $name');
    notifyListeners();
    await save();
  }

  /// The person stopped looking for the paired node, or connected.
  Future<void> setStopped(bool s) async {
    if (stopped == s) return;
    stopped = s;
    notifyListeners();
    await save();
  }

  void _badValue(Object e) =>
      _tell('The node sent a value this app cannot read: $e');

  void _tell(String s) {
    notice = s;
    note(s);
    notifyListeners();
  }

  void dismissNotice() {
    notice = null;
    notifyListeners();
  }

  /// Adds a line to the log.
  void note(String s) {
    final l = (at: now(), text: s);
    log.add(l);
    noted?.call(l);
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
    if (n.version >= clockVersion) await _setClock(link);
    if (n.version >= homeVersion) await _keepHome(link, n);
    return true;
  }

  /// Gives the person's home its key, silently, and reads Home. A home
  /// may have been reflashed, or the identity restored from its words;
  /// the node changes nothing when it holds that key already.
  Future<void> _keepHome(NodeLink link, NodeInfo n) async {
    try {
      final key = homeKey?.call(n);
      if (key != null) await link.setHome(key);
      nodeHome = await link.readHome();
      note(
        key != null
            ? 'Home: the key written to the person\'s home; $nodeHome'
            : 'Home: $nodeHome',
      );
    } catch (e) {
      note('Home: $e');
    }
    notifyListeners();
  }

  /// Writes Home, the mailbox key or none, and reads it back.
  Future<NodeHome> writeHome(List<int>? key) async {
    final link = _link;
    final n = node;
    if (link == null || n == null) throw StateError('not connected');
    if (n.version < homeVersion) {
      throw StateError('no Home before link version $homeVersion');
    }
    await link.setHome(key);
    nodeHome = await link.readHome();
    note('Home: wrote ${key == null ? 'no key' : 'the key'}, reads $nodeHome');
    notifyListeners();
    return nodeHome!;
  }

  /// Gives the node the phone's time and UTC offset, silently: a node
  /// without them only shows no time.
  Future<void> _setClock(NodeLink link) async {
    final t = now();
    try {
      await link.setClock(
        t.millisecondsSinceEpoch ~/ 1000,
        t.timeZoneOffset.inMinutes,
      );
      nodeTime = await link.readClock();
      note('Clock: ${describeTime(nodeTime!)}');
    } catch (e) {
      note('Clock: $e');
    }
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
    nodeTime = null;
    nodeHome = null;
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
    save();
  }

  /// Queues a message; the node takes it when ready.
  Future<void> queue(Outgoing o) async {
    outbox.add(o);
    notifyListeners();
    await save();
    _pump();
  }

  /// The longest a fetch waits for its home node's first reply, by the
  /// spec's bounds on a free channel: each way at most 256 hand-overs
  /// that succeed ("Hop count", "Bounds"), each at most 3.15 s with its
  /// attempts and retry waits ("Retry wait", "Delivery"), and at a sleepy
  /// requester's end its holder waits for its beat, at most the beat
  /// period of 300 s ("Beats", "Delivery"): 2 × 256 × 3.15 s + 300 s =
  /// 1912.8 s. Past it the fetch or its answer is taken as lost.
  static const fetchAnswerWait = Duration(
    milliseconds: 2 * 256 * 3150 + 300000,
  );

  /// The newest fetch while it waits for its answer: the node has not
  /// given it back or refused it, no reply of its round has come, and
  /// [fetchAnswerWait] has not passed since it was queued. Stored with
  /// the queue, so it is still waited for after a restart.
  Outgoing? get fetchWaiting {
    final o = outbox.where((o) => o.kind == OutKind.fetch).lastOrNull;
    if (o == null || o.acked) return null;
    if (o.state == OutState.givenBack || o.state == OutState.refused) {
      return null;
    }
    return now().difference(o.at) < fetchAnswerWait ? o : null;
  }

  /// Seconds left of the wait for [fetchWaiting]'s answer.
  int fetchWaitLeftS(Outgoing o) {
    final left = fetchAnswerWait - now().difference(o.at);
    return left.isNegative ? 0 : (left.inMilliseconds + 999) ~/ 1000;
  }

  /// What the home node's answer said, to tell the person once.
  String? fetchNews;

  /// A reply of the fetch of `round` came: the fetch is answered.
  Future<void> fetchAnswered(int round, {required bool nothingNew}) async {
    final o = outbox
        .where((o) => o.kind == OutKind.fetch && o.sent == round)
        .firstOrNull;
    if (nothingNew) {
      note('Fetch $round: no new mail');
      fetchNews = 'No new mail';
    }
    if (o == null || o.acked) {
      notifyListeners();
      return;
    }
    o.acked = true;
    notifyListeners();
    await save();
  }

  /// Keeps a received message.
  Future<void> addReceived(Received r) async {
    received.add(r);
    notifyListeners();
    kept?.call(r);
    await save();
  }

  /// Offers a given-back message again; the node takes it when ready.
  Future<void> offerAgain(Outgoing o) async {
    o.state = OutState.queued;
    o.retryAt = null;
    notifyListeners();
    await save();
    _pump();
  }

  Future<void> drop(Outgoing o) async {
    if (o.state == OutState.writing) return;
    outbox.remove(o);
    notifyListeners();
    await save();
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
    await save();
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

  /// Keeps a message before it is confirmed. One read again, after a
  /// confirmation that was lost, is not kept twice.
  Future<void> _keep(InboxMessage m) async {
    final read = reader;
    if (read != null) return read(m);
    final dup = received.any(
      (r) => r.id == m.msgId && listEquals(r.payload, m.payload),
    );
    if (dup) return;
    await addReceived(Received(m.msgId, m.payload, now()));
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

/// The node's time as a person reads it: UTC, the source, the offset.
String describeTime(NodeTime t) {
  final o = t.offsetMin;
  final sign = o < 0 ? '-' : '+';
  final offset =
      'UTC$sign${(o.abs() ~/ 60).toString().padLeft(2, '0')}:'
      '${(o.abs() % 60).toString().padLeft(2, '0')}';
  if (t.source == TimeSource.none) return 'no time, offset $offset';
  final utc = DateTime.fromMillisecondsSinceEpoch(t.utcS * 1000, isUtc: true);
  return '${utc.toIso8601String()} from the ${t.source.name}, offset $offset';
}
