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
  /// The node they were last known at, from their messages or their
  /// card (roadmap D162).
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

  /// Its number and the count of parts, for a part of a split text; 1 of
  /// 1 otherwise.
  final int part;
  final int of;

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
    this.part = 1,
    this.of = 1,
    this.acked = false,
    this.state = OutState.queued,
  }) : id = msgId(dst, payload);

  /// The `sent` of its text's first part, which names the text.
  int get first => sent - part + 1;

  String get text => switch (kind) {
    OutKind.text => body ?? '',
    OutKind.ack => 'Got it',
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
    'part': part,
    'of': of,
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
          part: j['part'] as int? ?? 1,
          of: j['of'] as int? ?? 1,
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

  /// Its number and the count of parts, for a part of a split text; 1 of
  /// 1 otherwise.
  final int part;
  final int of;

  /// It came as a first message, which anyone holding the person's card
  /// and its sender's could have made.
  final bool intro;

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
    this.part = 1,
    this.of = 1,
    this.intro = false,
    this.acked = false,
  }) : payload = Uint8List.fromList(payload);

  String get text => body ?? '(did not open) ${hex(payload)}';

  /// The `sent` of its text's first part, which names the text.
  int? get first => switch (sent) {
    final s? => s - part + 1,
    null => null,
  };

  /// Whether `o` is a part of the same text from the same sender.
  bool sameText(Received o) =>
      from != null && o.from == from && o.first == first && o.of == of;

  Map<String, dynamic> toJson() => {
    'id': id,
    'payload': hex(payload),
    'at': at.toIso8601String(),
    'from': from,
    'sent': sent,
    'body': body,
    'mailbox': mailbox,
    'part': part,
    'of': of,
    'intro': intro,
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
    part: j['part'] as int? ?? 1,
    of: j['of'] as int? ?? 1,
    intro: j['intro'] as bool? ?? false,
    acked: j['acked'] as bool,
  );
}

/// The node's neighbour table: its noise floor, its own position, zero
/// while it has none, its neighbours, and when it was read.
typedef NeighbourTable = ({
  int floorDbm,
  Position own,
  List<Neighbour> list,
  DateTime at,
});

/// A line of the link's event log.
typedef LogLine = ({DateTime at, String text});

/// A log line's local time of day, to the millisecond.
String logTime(DateTime at) => at.toLocal().toIso8601String().substring(11, 23);

/// How long the app waits for the link to end after an unpairing write
/// failed: the longest supervision timeout, 32 s (Bluetooth Core
/// Specification 5.0, Vol 6 Part B §4.5.2), by which a link the node
/// ended is over for the phone too.
const Duration unpairEnd = Duration(seconds: 32);

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

  /// The person's recipient tag at a node, which the app writes to it as
  /// it connects (spec "Phone link", Public).
  int Function(NodeInfo)? recipient;

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

  /// Whether the node serves phones that have not paired, as last read,
  /// from link version 7; the paired phone's alone to read.
  bool? nodePublic;

  /// The link is a guest's, on a public node that is not the person's:
  /// only Node, Status, Send, Inbox, Given back and Recipient may be
  /// touched, as Android may start a pairing by itself on a read or write
  /// the node refuses for want of the bond (spec "Phone link", Public).
  bool guest = false;

  /// The node's neighbour table as last read, from link version 4.
  NeighbourTable? neighbours;
  Future<NeighbourTable>? _readingNeighbours;

  /// What each neighbour answered when last asked, by its ID in hex,
  /// from link version 9.
  final Map<String, NeighbourStatus> answers = {};

  /// When each of [answers] came.
  final Map<String, DateTime> _answeredAt = {};

  /// The IDs, in hex, of the neighbours the running look waits for, and
  /// its wait, which ends early once none is left.
  final Set<String> _awaited = {};
  Completer<void>? _waiting;
  Timer? _waitTimer;

  /// Counts the looks, so that a look that has been replaced or stopped
  /// asks no more.
  int _look = 0;
  final _statusSeen = StreamController<Status>.broadcast();

  /// Each status the node gives, read or notified: when what it knows
  /// may have changed.
  Stream<Status> get statusSeen => _statusSeen.stream;
  DateTime? _statusAt;

  /// The last thing worth telling the person.
  String? notice;

  final List<Outgoing> outbox = [];
  final List<Received> received = [];

  /// The nodes this phone is paired with, by name, the one last
  /// connected last. A node keeps one phone, but a phone may be paired
  /// with many, and Android keeps a bond for each.
  final Set<String> pairedWith = {};

  /// The nodes of [pairedWith] that no longer hold this phone's bond:
  /// another phone has paired with each since (spec "Phone link", the
  /// replaced phone).
  final Set<String> pairingLost = {};

  /// The person closed the link or stopped looking for the paired node:
  /// the app does not look for it until they connect. Kept, so that the
  /// app started with the phone does not look either.
  bool stopped = false;

  bool _writing = false;
  bool _draining = false;
  final List<StreamSubscription<dynamic>> _subs = [];

  bool get connected => _link != null;

  /// The link while connected, for the firmware update (`updater.dart`).
  NodeLink? get link => _link;

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
        // A file from before a phone kept several held one name and
        // whether it was lost.
        pairedWith: switch (j['pairedWith']) {
          final String one => [one],
          final List<dynamic> many => many.cast<String>(),
          _ => const <String>[],
        },
        pairingLost: switch (j['pairingLost']) {
          true when j['pairedWith'] is String => [j['pairedWith'] as String],
          final List<dynamic> many => many.cast<String>(),
          _ => const <String>[],
        },
        stopped: j['stopped'] as bool? ?? false,
        guestAfter: {
          for (final e
              in (j['guestAfter'] as Map<String, dynamic>? ?? {}).entries)
            e.key: e.value as int,
        },
      ),
    );
    if (j == null) return;
    outbox.addAll(j.outbox);
    received.addAll(j.received);
    pairedWith.addAll(j.pairedWith);
    pairingLost.addAll(j.pairingLost);
    stopped = j.stopped;
    guestAfter.addAll(j.guestAfter);
    _retryWhenDue();
    notifyListeners();
  }

  Future<void> save() => store.save(version, {
    'outbox': [for (final o in outbox) o.toJson()],
    'received': [for (final r in received) r.toJson()],
    'pairedWith': [...pairedWith],
    'pairingLost': [...pairingLost],
    'stopped': stopped,
    'guestAfter': guestAfter,
  });

  /// The node `name` holds this phone's bond, or no longer does; the
  /// other nodes paired with stay.
  Future<void> setPaired(String name, {bool lost = false}) async {
    if (pairedWith.lastOrNull == name && pairingLost.contains(name) == lost) {
      return;
    }
    pairedWith
      ..remove(name)
      ..add(name);
    if (lost) {
      pairingLost.add(name);
    } else {
      pairingLost.remove(name);
    }
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
  /// note: Node first, refused if newer than the app; the recipient tag,
  /// so that the node hands the inbox over; then the subscriptions, Status
  /// and Given back, the inbox, the queue. A `guest` link stops there.
  Future<bool> attach(NodeLink link, {bool guest = false}) async {
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
        'This node\'s firmware is newer than this app understands: update '
        'the app.',
      );
      return false;
    }
    node = n;
    _link = link;
    this.guest = guest;
    guestRead = false;
    notice = null;
    note(
      'Node: ${nodeName(n.id)}, link version ${n.version}, image ${n.image}'
      '${guest ? ', as a guest' : ''}',
    );
    _subs
      ..add(link.statusNotified.listen(_onStatus, onError: _badValue))
      ..add(link.givenBackNotified.listen(_onGivenBack, onError: _badValue))
      ..add(link.answerNotified.listen(_onAnswer, onError: _badValue));
    unawaited(link.closed.then((_) => _detach(link)));
    if (n.version >= publicVersion) await _setRecipient(link, n);
    try {
      await link.subscribe();
      _onStatus(await link.readStatus());
      _onGivenBack(await link.readGivenBack());
    } catch (e) {
      _tell('Reading the node failed: $e');
    }
    if (guest) {
      guestRead = n.version >= guestVersion && await _readGuest(link, n);
      return true;
    }
    if (n.version >= clockVersion) await _setClock(link);
    if (n.version >= homeVersion) await _keepHome(link, n);
    if (n.version >= publicVersion) await _readPublic(link);
    if (n.version >= askVersion) await _subscribeAsk(link);
    return true;
  }

  /// Whether the guest link read every item that waits for the person at
  /// its node.
  bool guestRead = false;

  /// The number of the last item taken from each public node's ring, by
  /// node ID in hex: the node keeps nothing for a guest, so the phone
  /// keeps where it is (spec "Phone link", Public).
  final Map<String, int> guestAfter = {};

  /// Reads the items that wait for the person in the node's ring, oldest
  /// first, each after the last taken; the number goes on after each kept,
  /// so one lost with the link is read again, and kept once. Whether it
  /// read all.
  Future<bool> _readGuest(NodeLink link, NodeInfo n) async {
    final key = hex(n.id);
    try {
      while (_link == link) {
        final after = guestAfter[key] ?? 0;
        await link.setGuestAfter(after);
        final item = await link.readGuest();
        if (item == null) return true;
        // Any other number is taken: a node whose ring restarted reads a
        // number above its newest as 0. The one just written again would
        // be read in a loop.
        if (item.number == after) {
          note('Guest: item ${item.number} again');
          return false;
        }
        note(
          'Guest: item ${item.number}, ${msgIdHex(item.message.msgId)}, '
          '${item.message.payload.length} bytes',
        );
        await _keep(item.message);
        guestAfter[key] = item.number;
        await save();
      }
    } catch (e) {
      note('Guest: $e');
    }
    return false;
  }

  /// Messages wait to be offered to the node, or one is being written.
  bool get toSend => outbox.any(
    (o) => o.state == OutState.queued || o.state == OutState.writing,
  );

  /// Writes the person's recipient tag at the node: a public node hands a
  /// phone only the messages that carry its tag, and keeps the paired
  /// phone's to place its messages while it is away.
  Future<void> _setRecipient(NodeLink link, NodeInfo n) async {
    final tag = recipient?.call(n);
    if (tag == null) return;
    try {
      await link.setRecipient(tag);
      note('Recipient: tag $tag');
    } catch (e) {
      note('Recipient: $e');
    }
  }

  Future<void> _readPublic(NodeLink link) async {
    try {
      nodePublic = await link.readPublic();
      note('Public: ${nodePublic! ? 'yes' : 'no'}');
    } catch (e) {
      note('Public: $e');
    }
    notifyListeners();
  }

  /// The link, if the paired phone's and of `version` or later.
  NodeLink _owned(int version, String what) {
    final link = _link;
    final n = node;
    if (link == null || n == null) throw StateError('not connected');
    if (guest) throw StateError('$what is the paired phone\'s alone');
    if (n.version < version) {
      throw StateError('no $what before link version $version');
    }
    return link;
  }

  /// Makes the node public, or the person's alone, and reads it back.
  Future<bool> writePublic(bool public) async {
    final link = _owned(publicVersion, 'Public');
    await link.setPublic(public);
    nodePublic = await link.readPublic();
    note('Public: wrote ${public ? 'yes' : 'no'}, reads ${nodePublic!}');
    notifyListeners();
    return nodePublic!;
  }

  /// Has the connected node forget this phone's bond, and forgets the
  /// node here, the others paired with staying: the app no longer looks
  /// for it (spec "Phone link", Public). Throws
  /// only when the write failed and the link stayed up: the node then
  /// keeps the phone.
  Future<void> unpair() async {
    final link = _owned(unpairVersion, 'Unpairing');
    final name = nodeName(node!.id);
    try {
      await link.unpair();
    } catch (e) {
      // The node answers the write, then ends the link, and Android may
      // report the write failed as that end overtakes the answer, with its
      // catch-all GATT error 133, as at a reboot (`updater.dart`): a link
      // that ends is the node having forgotten the phone.
      if (!await _ends(link)) rethrow;
      note('Unpairing: the link ended under the write ($e), taken as done');
    }
    pairedWith.remove(name);
    pairingLost.remove(name);
    note('Pairing: unpaired from $name');
    notifyListeners();
    await save();
    await link.close();
  }

  /// Whether `link` ends within [unpairEnd].
  static Future<bool> _ends(NodeLink link) =>
      link.closed.then((_) => true).timeout(unpairEnd, onTimeout: () => false);

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
    final link = _owned(homeVersion, 'Home');
    await link.setHome(key);
    nodeHome = await link.readHome();
    note('Home: wrote ${key == null ? 'no key' : 'the key'}, reads $nodeHome');
    notifyListeners();
    return nodeHome!;
  }

  /// Reads the node's neighbour table, page by page (spec "Phone link",
  /// Neighbours); a read asked for while one runs gets that one.
  Future<NeighbourTable> readNeighbours() => _readingNeighbours ??=
      _readNeighbours().whenComplete(() => _readingNeighbours = null);

  Future<NeighbourTable> _readNeighbours() async {
    final link = _owned(neighboursVersion, 'Neighbours');
    final list = <Neighbour>[];
    var slot = 0;
    NeighbourPage page;
    do {
      await link.setNeighboursFrom(slot);
      page = await link.readNeighbours();
      list.addAll(page.neighbours);
      // 0 ends the table. Slots only grow, so a node that named one
      // already read would otherwise loop.
      if (page.next <= slot) break;
      slot = page.next;
    } while (true);
    final t = (floorDbm: page.floorDbm, own: page.own, list: list, at: now());
    if (_link == link) neighbours = t;
    note('Neighbours: ${list.length}, noise floor ${t.floorDbm} dBm');
    return t;
  }

  /// Subscribes to Ask with the others, at every connection: an answer
  /// comes within a second of its ask, and a subscription first made by
  /// the map would be one more write before the first ask. The node
  /// notifies only answers to asks, so it costs nothing while the map is
  /// shut.
  Future<void> _subscribeAsk(NodeLink link) async {
    try {
      await link.subscribeAsk();
    } catch (e) {
      note('Ask: $e');
    }
  }

  /// Asks for its status each of `list` that has not answered within
  /// [askFresh], and asks again, a round at a time, each that has not
  /// answered within [answerWait] of its round, up to [askRounds] asks
  /// (spec "Neighbour status"). An ask and its answer are single frames
  /// on a shared channel and are lost under load, so the one who looks
  /// asks again, not the node. A look that starts while another runs
  /// replaces it.
  Future<void> askNeighbours(List<Neighbour> list) async {
    final link = _owned(askVersion, 'Ask');
    stopAsking();
    final look = _look;
    final t = now();
    final stale = [
      for (final n in list)
        if (!(_answeredAt[hex(n.id)]?.isAfter(t.subtract(askFresh)) ?? false))
          n,
    ];
    for (final n in stale) {
      answers.remove(hex(n.id));
      _answeredAt.remove(hex(n.id));
    }
    notifyListeners();
    _awaited.addAll(stale.map((n) => hex(n.id)));
    for (var round = 0; round < askRounds && _awaited.isNotEmpty; round++) {
      final asked = [
        for (final n in stale)
          if (_awaited.contains(hex(n.id))) n,
      ];
      for (final n in asked) {
        try {
          await link.ask(n.id);
        } catch (e) {
          note('Ask: ${nodeName(n.id)}: $e');
          if (_link != link) {
            stopAsking();
            return;
          }
        }
        if (_look != look) return;
      }
      note('Ask: asked ${asked.length} neighbours, round ${round + 1}');
      final w = _waiting = Completer<void>();
      _waitTimer = Timer(answerWait, () {
        if (!w.isCompleted) w.complete();
      });
      if (_awaited.isEmpty) w.complete();
      await w.future;
      if (_look != look) return;
    }
    if (_awaited.isNotEmpty) {
      note('Ask: ${_awaited.length} neighbours did not answer');
    }
    stopAsking();
  }

  /// Ends the running look, if any: the person stopped looking, or a new
  /// look starts.
  void stopAsking() {
    _look++;
    _awaited.clear();
    _waitTimer?.cancel();
    _waitTimer = null;
    final w = _waiting;
    _waiting = null;
    if (w != null && !w.isCompleted) w.complete();
  }

  void _onAnswer(Answer? a) {
    if (a == null) return;
    final key = hex(a.id);
    answers[key] = a.status;
    _answeredAt[key] = now();
    note('Ask: ${nodeName(a.id)} answers ${a.status.lines.join(', ')}');
    _awaited.remove(key);
    final w = _waiting;
    if (_awaited.isEmpty && w != null && !w.isCompleted) w.complete();
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
    nodePublic = null;
    guest = false;
    neighbours = null;
    answers.clear();
    _answeredAt.clear();
    stopAsking();
    note('Disconnected');
  }

  void _onStatus(Status s) {
    note(
      'Status: ready ${s.ready}, inbox ${s.inbox}, relays ${s.relays}, '
      'heard ${s.heard}, boot left ${s.bootLeftS} s',
    );
    status = s;
    _statusAt = now();
    _statusSeen.add(s);
    notifyListeners();
    // A guest's mail is the ring's, read through Guest.
    if (s.inbox > 0 && !guest) _drain();
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
    _retryWhenDue();
    notifyListeners();
    save();
  }

  /// Sends again, by itself, each message given back once the wait the
  /// node asked for is over: the node takes no other before then, and
  /// the wait is the network's word on when it can (spec "Phone link",
  /// Given back).
  Timer? _retry;

  void _retryWhenDue() {
    _retry?.cancel();
    final due = [
      for (final o in outbox)
        if (o.state == OutState.givenBack && o.retryAt != null) o.retryAt!,
    ];
    if (due.isEmpty) return;
    final first = due.reduce((a, b) => a.isBefore(b) ? a : b);
    final wait = first.difference(now());
    _retry = Timer(wait.isNegative ? Duration.zero : wait, () {
      final t = now();
      for (final o in outbox) {
        final at = o.retryAt;
        if (o.state == OutState.givenBack && at != null && !at.isAfter(t)) {
          o.state = OutState.queued;
          o.retryAt = null;
          note('Send ${msgIdHex(o.id)}: its wait is over, sent again');
        }
      }
      notifyListeners();
      unawaited(save());
      _pump();
      _retryWhenDue();
    });
  }

  /// Queues a message; the node takes it when ready.
  Future<void> queue(Outgoing o) async {
    outbox.add(o);
    notifyListeners();
    await save();
    _pump();
  }

  /// How long after its round's asks a neighbour is waited for before it
  /// is asked again. On a quiet channel an answer comes about 100 ms after
  /// the ask (the bench, 2026-11-09); under load each of the two frames
  /// may wait two backoffs of up to 200 ms and three checks of 3.1 ms
  /// (spec "Heartbeat"), and to a sleepy node they carry the long
  /// preamble, 249.92 and 257.60 ms on air (spec "Neighbour status"):
  /// 2 × 409.3 ms + 507.52 ms = 1.33 s at most more. 2 s leaves the phone
  /// link and the node's other frames room.
  static const answerWait = Duration(seconds: 2);

  /// Asks of a neighbour in one look, the first and two more.
  static const askRounds = 3;

  /// An answer younger than this is not asked again when the map is
  /// refreshed: a choice, the mean heartbeat interval (spec "Heartbeat"),
  /// within which what a neighbour hears seldom changes.
  static const askFresh = Duration(seconds: 10);

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

  /// A reply of the fetch of `round` came: the fetch is answered. The
  /// first to come tells the person what `remaining` the home holds
  /// beyond its batch, so that they can fetch again.
  Future<void> fetchAnswered(
    int round, {
    required bool nothingNew,
    int remaining = 0,
  }) async {
    final o = outbox
        .where((o) => o.kind == OutKind.fetch && o.sent == round)
        .firstOrNull;
    if (nothingNew) {
      note('Fetch $round: no new mail');
      fetchNews = 'No new mail';
    } else if (remaining > 0 && o != null && !o.acked) {
      note('Fetch $round: $remaining more at home');
      fetchNews = '$remaining more at home: fetch again for them';
    }
    if (o == null || o.acked) {
      notifyListeners();
      return;
    }
    o.acked = true;
    notifyListeners();
    await save();
  }

  /// The parts of `r`'s text that have come, by number from 1, null
  /// where one has not.
  List<Received?> partsOf(Received r) {
    final parts = List<Received?>.filled(r.of, null);
    for (final x in received) {
      if (x.body != null && (identical(x, r) || x.sameText(r))) {
        parts[x.part - 1] = x;
      }
    }
    return parts;
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
    if (link == null || guest) return;
    try {
      await link.setMapPoint(p);
      note('Map point: ${p ?? 'cleared'}');
    } catch (e) {
      _tell('Setting the node\'s place failed: $e');
    }
  }

  @override
  void dispose() {
    _retry?.cancel();
    for (final s in _subs) {
      s.cancel();
    }
    _statusSeen.close();
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
