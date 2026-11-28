// The app's side of the phone link (spec "Phone link"): the phone is the
// queue, it drains the node's inbox, and it matches what the node gives
// back to the messages it sent.

import 'dart:async';
import 'dart:collection';

import 'package:flutter/foundation.dart';

import 'asks.dart';
import 'l10n.dart';
import 'link.dart';
import 'node_link.dart';
import 'nodes.dart';
import 'store.dart';

enum OutState {
  /// Waiting in the app for the node to be ready.
  queued,

  /// Written; the node has not answered yet.
  writing,

  /// The node has it.
  taken,

  /// The node gave it back, for [Outgoing.why]: offered again by itself
  /// once the node can take it, busy or with no neighbour, and for no way
  /// only on the person's word (spec "Phone link", Given back).
  givenBack,

  /// The node will never take it as written: it answered the write with
  /// an error other than not ready.
  refused,
}

/// What a sent message carries for the person.
enum OutKind {
  /// A text.
  text,

  /// A reaction to a received text.
  reaction,

  /// The person's card.
  card,

  /// A fetch from the person's mailbox.
  fetch,

  /// The person's home, given to a contact.
  home,
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

  /// The contact's identity in hex; for a fetch, the contact a command
  /// on it is about, if any.
  final String? to;

  /// A fetch's command (spec "Mailbox", The owner's commands), by which
  /// its summary's outcome is read.
  final Uint8List? command;

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

  /// A fetch: a reply of its round came from the home node.
  bool answered;

  /// A text: the other side's reaction, one emoji, and the `sent` of the
  /// message that carried it, so that an older one coming late does not
  /// replace it.
  String? reaction;
  int reactionSent;
  OutState state;

  /// Why the node gave it back, while [state] is [OutState.givenBack].
  Why? why;
  DateTime? retryAt;
  String? error;

  /// The IDs of the parts of a text this one's text was sent again in
  /// place of (`Messenger.sendAgain`): one may yet come back, and is no
  /// stranger's.
  List<int> replaced;

  Outgoing(
    this.dst,
    this.payload, {
    required this.at,
    required this.kind,
    required this.sent,
    this.to,
    this.command,
    this.body,
    this.toward,
    this.part = 1,
    this.of = 1,
    this.answered = false,
    this.reaction,
    this.reactionSent = 0,
    this.state = OutState.queued,
    this.why,
    this.replaced = const [],
  }) : id = msgId(dst, payload);

  /// Given back because its search found no way: it waits for the person.
  bool get noWay => state == OutState.givenBack && why == Why.noWay;

  /// Leaves the given-back state to be offered to the node again.
  void requeue() {
    state = OutState.queued;
    why = null;
    retryAt = null;
  }

  /// The `sent` of its text's first part, which names the text.
  int get first => sent - part + 1;

  String get text => switch (kind) {
    OutKind.text => body ?? '',
    OutKind.reaction => 'Reaction ${body ?? ''}',
    OutKind.card => 'Your card',
    OutKind.fetch => 'Fetch from your mailbox',
    OutKind.home => 'A home at your node',
  };

  Map<String, dynamic> toJson() => {
    'dst': dst.toString(),
    'payload': hex(payload),
    'at': at.toIso8601String(),
    'kind': kind.name,
    'to': to,
    if (command case final c?) 'command': hex(c),
    'sent': sent,
    'body': body,
    'toward': toward?.name,
    'part': part,
    'of': of,
    'answered': answered,
    'reaction': reaction,
    'reactionSent': reactionSent,
    // An answer the app did not hear may have been a take; offered again,
    // the same message has the same ID and is one message to every node.
    'state': (state == OutState.writing ? OutState.queued : state).name,
    if (why case final w?) 'why': w.name,
    'retryAt': retryAt?.toIso8601String(),
    'error': error,
    if (replaced.isNotEmpty) 'replaced': replaced,
  };

  static Outgoing fromJson(Map<String, dynamic> j) {
    final kind = OutKind.values.byName(j['kind'] as String);
    final state = OutState.values.byName(j['state'] as String);
    return Outgoing(
        Address.parse(j['dst'] as String)!,
        parseHex(j['payload'] as String)!,
        at: DateTime.parse(j['at'] as String),
        kind: kind,
        to: j['to'] as String?,
        // Absent from a fetch without one, and from every other kind.
        command: switch (j['command']) {
          null => null,
          final c => parseHex(c as String)!,
        },
        sent: j['sent'] as int,
        body: j['body'] as String?,
        toward: j['toward'] == null
            ? null
            : Toward.values.byName(j['toward'] as String),
        part: j['part'] as int,
        of: j['of'] as int,
        answered: j['answered'] as bool,
        reaction: j['reaction'] as String?,
        reactionSent: j['reactionSent'] as int,
        state: state,
        // Before the node said why, every message came back busy.
        why: state != OutState.givenBack
            ? null
            : Why.values.byName(j['why'] as String? ?? Why.busy.name),
        replaced: [for (final i in j['replaced'] as List? ?? []) i as int],
      )
      ..retryAt = j['retryAt'] == null
          ? null
          : DateTime.parse(j['retryAt'] as String)
      ..error = j['error'] as String?;
  }
}

/// A received text, or a payload that did not open: kept for the debug
/// screen, in no conversation.
class Received {
  final int id;
  final Uint8List payload;
  final DateTime at;

  /// The sender's identity in hex; null if the payload did not open.
  final String? from;

  /// Its `sent`, by which a reaction names it.
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

  /// The person's reaction to it, one emoji.
  String? reaction;

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
    this.reaction,
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
    'reaction': reaction,
  };

  static Received fromJson(Map<String, dynamic> j) => Received(
    j['id'] as int,
    parseHex(j['payload'] as String)!,
    DateTime.parse(j['at'] as String),
    from: j['from'] as String?,
    sent: j['sent'] as int?,
    body: j['body'] as String?,
    mailbox: j['mailbox'] as bool,
    part: j['part'] as int,
    of: j['of'] as int,
    intro: j['intro'] as bool,
    reaction: j['reaction'] as String?,
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

/// Something worth telling the person; `about` names the node it is
/// about, when it says that node cannot be used, as the phone may then
/// be on another: it stays until dismissed or a link to that node is
/// attached. Any other goes at the next link attached.
typedef Notice = ({String text, String? about});

/// A line of the link's event log.
typedef LogLine = ({DateTime at, String text});

/// The link's events since the app started, newest last; the oldest go
/// once there are [lines], more than a bench session scrolls back through.
class EventLog extends ChangeNotifier with IterableMixin<LogLine> {
  static const lines = 500;
  final _all = ListQueue<LogLine>();

  @override
  Iterator<LogLine> get iterator => _all.iterator;

  void add(LogLine l) {
    _all.add(l);
    if (_all.length > lines) _all.removeFirst();
    notifyListeners();
  }
}

/// A log line's local time of day, to the millisecond.
String logTime(DateTime at) => at.toLocal().toIso8601String().substring(11, 23);

/// How long the app waits for the link to end after an unpairing write
/// failed: the longest supervision timeout, 32 s (Bluetooth Core
/// Specification 5.0, Vol 6 Part B §4.5.2), by which a link the node
/// ended is over for the phone too.
const Duration unpairEnd = Duration(seconds: 32);

/// The messages: the queue, what was received, and the reactions the
/// person used most.
typedef Messages = ({
  List<Outgoing> outbox,
  List<Received> received,
  Map<String, int> reactionsUsed,
});

/// What the phone keeps besides the messages: the nodes, and whether the
/// person stopped looking ([Session.stopped]).
typedef _State = ({List<NodeRecord> nodes, bool stopped});

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

  /// The person's name as their card has it, which the app writes to
  /// Recipient beside the tag, so that the node's other phones see who is
  /// paired (spec "Phone link", Pairing).
  String Function()? name;

  /// The last value of Given back read or notified.
  GivenBack? givenBack;

  /// The link's events since the app started ([EventLog]).
  final EventLog log = EventLog();

  /// Told of each log line as it is added.
  void Function(LogLine)? noted;

  NodeLink? _link;
  NodeInfo? node;
  Status? status;

  /// The node's time as last read, after the app wrote its own.
  NodeTime? nodeTime;

  /// The node's Home as last read.
  NodeHome? nodeHome;

  /// The node's records of phones as last read; the paired phones' alone
  /// to read (spec "Phone link", Phones).
  List<PhoneRecord>? phones;

  /// Status's `phones` as the last read of [phones] began; null before the
  /// first on this link.
  int? _phonesAt;

  /// The read of [phones] under way, on [_phonesLink], and whether it is
  /// to read them again.
  Future<void>? _readingPhones;
  NodeLink? _phonesLink;
  bool _phonesAgain = false;

  /// Whether the node serves phones that have not paired, as last read;
  /// the paired phone's alone to read.
  bool? nodePublic;

  /// The link is a guest's, on a public node that is not the person's:
  /// only Node, Status, Send, Inbox, Given back and Recipient may be
  /// touched, as Android may start a pairing by itself on a read or write
  /// the node refuses for want of the bond (spec "Phone link", Public).
  bool guest = false;

  /// The node's neighbour table as last read.
  NeighbourTable? neighbours;
  Future<NeighbourTable>? _readingNeighbours;

  /// What each neighbour answered when last asked, by its ID in hex.
  final Map<String, NeighbourStatus> answers = {};

  /// Whom to ask, and when.
  final Asks _asks = Asks();

  /// Wakes [_askDue] when a node's wait for its answer is over.
  Timer? _askTimer;

  /// The asks being written, one after another.
  Future<void> _asking = Future.value();
  final _statusSeen = StreamController<Status>.broadcast();

  /// Each status the node gives, read or notified: when what it knows
  /// may have changed.
  Stream<Status> get statusSeen => _statusSeen.stream;
  DateTime? _statusAt;

  /// The last thing worth telling the person.
  Notice? notice;

  final List<Outgoing> outbox = [];
  final List<Received> received = [];

  /// What the phone knows of each node; kept with the queue, as whether
  /// the phone is paired with a node and where it stands in a node's
  /// ring outlive the app's run.
  final NodeStore nodes = NodeStore();

  /// The person closed the link or stopped looking for the paired node:
  /// the app does not look for it until they connect. Kept, so that the
  /// app started with the phone does not look either.
  bool stopped = false;

  bool _writing = false;
  bool _draining = false;

  /// Given back was read on this link; nothing is written before.
  bool _givenBackRead = false;
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

  /// The version of `bark.json`'s layout: since 4 a log ([Store.loadLog])
  /// of the changes below, so that a change costs what changed; 3 held
  /// the whole state as one object, rewritten at every change, which
  /// [load] still reads.
  ///
  /// Each line is a list of changes, kept whole or not at all:
  /// `{"out": k, "o": {…}}` a sent message `k`, new or changed, and
  /// `{"out": k}` one dropped; `{"in": k, "r": {…}}` a received one,
  /// new or changed; `{"state": {…}}` the nodes, [stopped] and
  /// [reactionsUsed]. The messages' own forms are [Outgoing.toJson] and
  /// [Received.toJson], which an identity file carries too ([Messages]).
  static const version = 4;

  /// The versions whose messages are in this app's forms: a file of 3
  /// differs only in how it is laid out.
  static const messagesRead = {3, version};

  /// Each message's number in the file.
  final _keys = Expando<int>();
  int _lastKey = 0;
  int _key(Object m) => _keys[m] ??= ++_lastKey;

  /// The changes in the file, since it was last written whole.
  int _inFile = 0;

  /// The file is written whole at the next change: there is none of this
  /// version yet, or an append failed and may have left half a line.
  bool _rewriteNext = true;

  /// The changes that rewriting the file would write: one a message, and
  /// the state.
  int get _live => outbox.length + received.length + 1;

  Future<void> load() async {
    final r = await store.loadLog(
      version,
      _replay,
      older: {3: (j) => (messages: messagesOf(j), state: _stateOf(j))},
    );
    if (r == null) return;
    final (:messages, :state) = r.value;
    outbox.addAll(messages.outbox);
    received.addAll(messages.received);
    reactionsUsed.addAll(messages.reactionsUsed);
    nodes.load(state.nodes);
    stopped = state.stopped;
    _rewriteNext = r.afresh;
    _retryWhenDue();
    _fetchOverWhenDue();
    notifyListeners();
    if (_rewriteNext || _inFile > 2 * _live) await _rewrite();
  }

  /// The messages and the state the changes of a log make, in order;
  /// throws for a change it cannot read.
  ({Messages messages, _State state}) _replay(List<Object?> lines) {
    final out = <int, Outgoing>{};
    final ins = <int, Received>{};
    Map<String, dynamic>? state;
    var n = 0;
    for (final line in lines) {
      for (final c in line as List) {
        n++;
        switch (c) {
          case {'out': final int k, 'o': final Map<String, dynamic> o}:
            out[k] = Outgoing.fromJson(o);
          case {'out': final int k}:
            out.remove(k);
          case {'in': final int k, 'r': final Map<String, dynamic> r}:
            ins[k] = Received.fromJson(r);
          case {'state': final Map<String, dynamic> s}:
            state = s;
          default:
            throw FormatException('a change $c');
        }
      }
    }
    final kept = state;
    for (final MapEntry(:key, :value) in [...out.entries, ...ins.entries]) {
      _keys[value] = key;
      if (key > _lastKey) _lastKey = key;
    }
    _inFile = n;
    return (
      messages: (
        outbox: [...out.values],
        received: [...ins.values],
        reactionsUsed: kept == null ? {} : _reactionsOf(kept),
      ),
      state: kept == null ? (nodes: [], stopped: false) : _stateOf(kept),
    );
  }

  static _State _stateOf(Map<String, dynamic> j) => (
    nodes: [
      for (final e in j['nodes'] as List)
        NodeRecord.fromJson(e as Map<String, dynamic>),
    ],
    stopped: j['stopped'] as bool,
  );

  static Map<String, int> _reactionsOf(Map<String, dynamic> j) => {
    for (final e in (j['reactionsUsed'] as Map<String, dynamic>).entries)
      e.key: e.value as int,
  };

  /// The messages of `j`, a file of a version of [messagesRead] or an
  /// identity file's messages.
  static Messages messagesOf(Map<String, dynamic> j) => (
    outbox: [
      for (final e in j['outbox'] as List)
        Outgoing.fromJson(e as Map<String, dynamic>),
    ],
    received: [
      for (final e in j['received'] as List)
        Received.fromJson(e as Map<String, dynamic>),
    ],
    reactionsUsed: _reactionsOf(j),
  );

  /// `m` as [messagesOf] reads it, with this version.
  static Map<String, dynamic> messagesJson(Messages m) => {
    'version': version,
    'outbox': [for (final o in m.outbox) o.toJson()],
    'received': [for (final r in m.received) r.toJson()],
    'reactionsUsed': m.reactionsUsed,
  };

  Map<String, dynamic> _outChange(Outgoing o) => {
    'out': _key(o),
    'o': o.toJson(),
  };
  Map<String, dynamic> _inChange(Received r) => {
    'in': _key(r),
    'r': r.toJson(),
  };
  Map<String, dynamic> _stateChange() => {
    'state': {
      'nodes': nodes.toJson(),
      'stopped': stopped,
      'reactionsUsed': reactionsUsed,
    },
  };

  /// Saves what changed: the messages `out` and `received`, each in the
  /// queue or kept, and with `state` the nodes, [stopped] and
  /// [reactionsUsed]. One line of the file, which a kill at any moment
  /// keeps whole or loses whole.
  Future<void> save({
    Iterable<Outgoing> out = const [],
    Iterable<Received> received = const [],
    bool state = false,
  }) => _save([
    for (final o in out) _outChange(o),
    for (final r in received) _inChange(r),
    if (state) _stateChange(),
  ]);

  Future<void> _save(List<Map<String, dynamic>> changes) async {
    if (changes.isEmpty) return;
    _inFile += changes.length;
    // Written whole once the changes in the file outnumber those that
    // rewriting it would write, so that it stays within about twice its
    // contents and each change costs, over time, about two.
    if (_rewriteNext || _inFile > 2 * _live) return _rewrite();
    try {
      await store.append(Store.logLine(changes));
    } catch (_) {
      _rewriteNext = true;
      rethrow;
    }
  }

  /// Writes the file whole, in place of the old one at once.
  Future<void> _rewrite() async {
    final text = Store.logText(version, [
      for (final o in outbox) [_outChange(o)],
      for (final r in received) [_inChange(r)],
      [_stateChange()],
    ]);
    _inFile = _live;
    _rewriteNext = false;
    try {
      await store.write(text);
    } catch (_) {
      _rewriteNext = true;
      rethrow;
    }
  }

  /// Takes the nodes and the messages of an identity file, or no
  /// messages: those of another identity are no one's here now. Applied at
  /// once, saved after.
  Future<void> moveIn(
    Iterable<({String name, bool paired, int ringAfter})> ns, {
    List<Outgoing> outbox = const [],
    List<Received> received = const [],
    Map<String, int> reactionsUsed = const {},
  }) {
    this.outbox
      ..clear()
      ..addAll(outbox);
    this.received
      ..clear()
      ..addAll(received);
    this.reactionsUsed
      ..clear()
      ..addAll(reactionsUsed);
    nodes.moveIn(ns);
    fetchNews = null;
    _retryWhenDue();
    _fetchOverWhenDue();
    notifyListeners();
    return _rewrite();
  }

  /// The node `name` holds this phone's bond, or no longer does; the
  /// other nodes paired with stay.
  Future<void> setPaired(String name, {bool lost = false}) async {
    if (!nodes.setPaired(name, lost: lost)) return;
    note(lost ? 'Pairing: $name is no longer ours' : 'Pairing: $name');
    notifyListeners();
    await save(state: true);
  }

  /// The person stopped looking for the paired node, or connected.
  Future<void> setStopped(bool s) async {
    if (stopped == s) return;
    stopped = s;
    notifyListeners();
    await save(state: true);
  }

  void _badValue(Object e) => tellIn((w) => w.sessionBadValue(said(e, w)));

  /// Tells the person `s` in the notice, and logs it, or `log` in its
  /// place: the log stays English, as a fault report carries it.
  void tell(String s, {String? about, String? log}) {
    notice = (text: s, about: about);
    note(log ?? s);
    notifyListeners();
  }

  /// Tells the person `words` in the language spoken, and logs them in
  /// English.
  void tellIn(String Function(AppLocalizations) words) =>
      tell(words(tr), log: words(english));

  void dismissNotice() {
    notice = null;
    notifyListeners();
  }

  /// Adds a line to the log, which tells its own listeners alone: a line
  /// changes nothing the others show.
  void note(String s) {
    final l = (at: now(), text: s);
    log.add(l);
    noted?.call(l);
  }

  /// Takes a connected link through the checklist of the phone link's
  /// note: Node first; the recipient tag, so that the node hands the inbox
  /// over; then the subscriptions, Status and Given back, the inbox, the
  /// queue. A `guest` link stops there. A node of a later link version is
  /// used as one of this app's ([nodeNewer]). Whether the link is
  /// attached.
  Future<bool> attach(NodeLink link, {bool guest = false}) async {
    final NodeInfo n;
    try {
      n = await link.readNode();
    } catch (_) {
      await link.close();
      rethrow;
    }
    if (!servesEachOther(n)) {
      note(
        'Node: ${nodeName(n.id)}, link version ${n.version}, oldest '
        '${n.oldest}; this app $linkVersion, oldest $oldestNode',
      );
      await _notServed(link, n);
      return false;
    }
    node = n;
    _link = link;
    this.guest = guest;
    guestRead = false;
    final about = notice?.about;
    if (about == null || about == nodeName(n.id)) notice = null;
    nodes.of(nodeName(n.id)).unserved = false;
    note(
      'Node: ${nodeName(n.id)}, link version ${n.version}, image ${n.image}'
      '${nodeNewer ? ', newer than this app\'s $linkVersion' : ''}'
      '${guest ? ', as a guest' : ''}',
    );
    notifyListeners();
    _subs
      ..add(link.statusNotified.listen(_onStatus, onError: _badValue))
      ..add(link.givenBackNotified.listen(_onGivenBack, onError: _badValue))
      ..add(link.answerNotified.listen(_onAnswer, onError: _badValue));
    unawaited(link.closed.then((_) => _detach(link)));
    if (!await _setRecipient(link, n)) return false;
    try {
      // Before anything is written: the node empties Given back when it
      // takes the next message, and a queued one may be the one it names.
      try {
        _onGivenBack(await link.readGivenBack());
      } finally {
        if (_link == link) _givenBackRead = true;
      }
      await link.subscribe();
      _onStatus(await link.readStatus());
      // Changed before the subscription, it was not notified.
      _onGivenBack(await link.readGivenBack());
    } catch (e) {
      tellIn((w) => w.sessionReadFailed(said(e, w)));
    }
    if (guest) {
      guestRead = await _readGuest(link, n);
      return true;
    }
    await _setClock(link);
    await _readPhones(link);
    await _keepHome(link, n);
    await _readPublic(link);
    await _subscribeAsk(link);
    return true;
  }

  /// The node speaks a later link version than this app and still serves
  /// it, as its oldest says ([servesEachOther]): the app uses what it
  /// knows, and the person may update the app for the rest (spec "Phone
  /// link", Versions).
  bool get nodeNewer => (node?.version ?? 0) > linkVersion;

  /// Whether the guest link read every item that waits for the person at
  /// its node.
  bool guestRead = false;

  /// How many times the person has reacted with each emoji, the one used
  /// last at the end, for the picker's first row (`emojis.dart`).
  final Map<String, int> reactionsUsed = {};

  /// Counts a reaction the person sent; saved with the message that
  /// carries it, as the state.
  void usedReaction(String emoji) {
    final n = reactionsUsed.remove(emoji) ?? 0;
    reactionsUsed[emoji] = n + 1;
  }

  /// Reads the items that wait for the person in the node's ring, oldest
  /// first, each after the last taken; the number goes on after each kept,
  /// so one lost with the link is read again, and kept once. Whether it
  /// read all.
  Future<bool> _readGuest(NodeLink link, NodeInfo n) async {
    final r = nodes.of(nodeName(n.id));
    try {
      while (_link == link) {
        final after = r.ringAfter;
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
        r.ringAfter = item.number;
        await save(state: true);
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

  /// Something on the link would be cut were it closed now: a Send
  /// written and not yet answered, an inbox message read and not yet
  /// confirmed, or a fetch whose answer is awaited, which comes to the
  /// node it was sent from.
  bool get inFlight => _writing || _draining || fetchWaiting != null;

  /// Writes the person's recipient tag at the node: a public node hands a
  /// phone only the messages that carry its tag, and keeps the paired
  /// phone's to place its messages while it is away. It writes this app's
  /// versions with it, and a paired phone the person's name; a guest's
  /// name is left empty, as the node keeps nothing of a guest's. False
  /// when the node finds that it and the app do not serve each other,
  /// which [servesEachOther] tells before the write; the node then ends
  /// the link.
  Future<bool> _setRecipient(NodeLink link, NodeInfo n) async {
    final tag = recipient?.call(n);
    if (tag == null) return true;
    final who = guest ? '' : name?.call() ?? '';
    try {
      await link.setRecipient(tag, who);
      note('Recipient: tag $tag${who.isNotEmpty ? ', a name' : ''}');
    } on AttException catch (e) {
      if (e.code != AttError.incompatible) {
        note('Recipient: $e');
        return true;
      }
      await _notServed(link, n);
      return false;
    } catch (e) {
      note('Recipient: $e');
    }
    return true;
  }

  /// The person's name changed: a paired phone's link writes Recipient
  /// again, so that the node's other phones see the new one now and not
  /// at this phone's next link.
  Future<void> nameChanged() async {
    final link = _link, n = node;
    if (link == null || n == null || guest) return;
    await _setRecipient(link, n);
  }

  /// Closes `link` to `n`, which and this app do not serve each other,
  /// says so, and marks the node passed over ([NodeRecord.unserved]).
  Future<void> _notServed(NodeLink link, NodeInfo n) async {
    await link.close();
    final name = nodeName(n.id);
    nodes.of(name).unserved = true;
    tell(notServedLine(n), about: name, log: notServedLine(n, english));
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

  /// The link, if the paired phone's.
  NodeLink _owned(String what) {
    final link = _link;
    if (link == null || node == null) {
      throw NotOwned((w) => w.notConnected);
    }
    if (guest) throw NotOwned((w) => w.sessionPairedOnly(what));
    return link;
  }

  /// Makes the node public, or the person's alone, and reads it back.
  Future<bool> writePublic(bool public) async {
    final link = _owned('Public');
    await link.setPublic(public);
    nodePublic = await link.readPublic();
    note('Public: wrote ${public ? 'yes' : 'no'}, reads ${nodePublic!}');
    notifyListeners();
    return nodePublic!;
  }

  /// Has the connected node forget this phone's bond, and forgets the
  /// node here, the others paired with staying: the app no longer looks
  /// for it (spec "Phone link", Public). Throws only when the write failed
  /// and the link stayed up: the node then keeps the phone.
  Future<void> unpair() async {
    final link = _owned('Unpairing');
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
    nodes.unpaired(name);
    note('Pairing: unpaired from $name');
    notifyListeners();
    await save(state: true);
    await link.close();
  }

  /// Forgets the node `name`, no longer this phone's, the others paired
  /// with staying: the person lets it go (roadmap D173).
  Future<void> forget(String name) async {
    if (!nodes.unpaired(name)) return;
    note('Pairing: forgot $name');
    notifyListeners();
    await save(state: true);
  }

  /// Whether `link` ends within [unpairEnd].
  static Future<bool> _ends(NodeLink link) =>
      link.closed.then((_) => true).timeout(unpairEnd, onTimeout: () => false);

  /// Gives the person's home its key, silently, and reads Home. A home
  /// may have been reflashed, or the identity restored from its words;
  /// the node changes nothing when it holds that key already.
  ///
  /// The node says whether the home is this phone's person's, and the key
  /// goes silently only where a record with the person's tag holds a
  /// home: one their phone left by unpairing, which the write joins to
  /// this phone's, or another phone of theirs. With none, a phone paired
  /// with the node removed the home (spec "Phone link", Phones), and the
  /// person is told rather than overruled.
  Future<void> _keepHome(NodeLink link, NodeInfo n) async {
    try {
      var key = homeKey?.call(n);
      if (key != null) {
        final h = await link.readHome();
        final tag = recipient?.call(n);
        final kept = (phones ?? const <PhoneRecord>[]).any(
          (r) => r.home && !r.mine && r.tag != null && r.tag == tag,
        );
        if (h.mine || !kept) key = null;
        if (!h.mine && !kept) _homeTaken();
      }
      if (key != null) await link.setHome(await _homeSet(key));
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

  /// At this link the person's home said a phone paired with it removed
  /// their home there, as the link came up ([_keepHome]) or during it
  /// ([_readPhones]).
  bool homeRemoved = false;

  void _homeTaken() {
    homeRemoved = true;
    tellIn((w) => w.sessionHomeTaken);
  }

  /// Reads the node's records, page by page, once the read under way, if
  /// any, is done: a read asked for while one runs makes it read again,
  /// so the list ends as the node's newest.
  Future<void> _readPhones(NodeLink link) {
    _phonesAgain = true;
    final running = _readingPhones;
    if (running != null && _phonesLink == link) return running;
    if (_link != link) return Future.value();
    _phonesLink = link;
    return _readingPhones = _phonesLoop(link);
  }

  /// Reads the records until a read finds Status's `phones` where it was
  /// as the read began, so that no list is made of the pages of two. Once
  /// the screen shows the node, its Node and Home are read again too, as
  /// a node's room and whether it is anyone's home follow its records.
  Future<void> _phonesLoop(NodeLink link) async {
    try {
      while (_phonesAgain && _link == link) {
        _phonesAgain = false;
        _phonesAt = status?.phones;
        final all = <PhoneRecord>[];
        var from = 0;
        do {
          await link.setPhonesFrom(from);
          final page = await link.readPhones();
          all.addAll(page.records);
          // A next place at or before the last guards against a loop.
          from = page.next > from ? page.next : 0;
        } while (from != 0);
        // A change since the read began moves `phones`, and [_onStatus]
        // asks for another read.
        _onStatus(await link.readStatus());
        if (_phonesAgain || _link != link) continue;
        phones = all;
        note(
          'Phones: ${all.length} records, '
          '${all.where((r) => r.paired).length} paired, '
          '${all.where((r) => r.home).length} homes',
        );
        if (nodeHome != null) {
          final n = await link.readNode();
          if (_link == link && listEquals(n.id, node?.id)) node = n;
          final h = await link.readHome();
          if (_link == link) {
            // A phone paired with the node removed the home while this
            // one was linked.
            final taken = nodeHome!.mine && !h.mine && homeKey?.call(n) != null;
            nodeHome = h;
            if (taken) _homeTaken();
          }
        }
        notifyListeners();
      }
    } catch (e) {
      note('Phones: $e');
      notifyListeners();
    } finally {
      if (_phonesLink == link) _readingPhones = null;
    }
  }

  /// Reads the node's records again.
  Future<void> readPhones() => _readPhones(_owned('Phones'));

  /// Has the node draw a fresh code on its screen.
  Future<void> askCode() async {
    await _owned('Phones').askCode();
    note('Phones: a code asked for');
  }

  /// Makes `change` to record `r` with the `code` the node's screen
  /// shows, then reads the records and Home again. Throws [AttException]
  /// 0x81 for a code not taken, wrong or late, and 0x13 for a record or
  /// part the node no longer holds.
  Future<void> byCode(PhoneRecord r, RecordChange change, int code) async {
    final link = _owned('Phones');
    try {
      await link.byCode(r.number, change, code);
      note('Phones: record ${r.number}: ${change.name} by code');
    } finally {
      await _readPhones(link);
      try {
        nodeHome = await link.readHome();
      } catch (e) {
        note('Home: $e');
      }
      notifyListeners();
    }
  }

  /// The next `sent` of the person's, later than any given before.
  Future<int> Function()? nextSent;

  /// `key` with a fresh `sent`, so that every later fetch is later than
  /// it: the node takes none that is not (spec "Phone link", Settings).
  Future<HomeSet> _homeSet(List<int> key) async {
    final next = nextSent;
    if (next == null) throw StateError('no sent to write Home with');
    return (key: key, sent: await next());
  }

  /// Reads Home again, as the node may have forgotten a key since the
  /// connection began; null when no node is linked or it gave none.
  Future<NodeHome?> readHomeAgain() async {
    final link = _link;
    if (link == null || nodeHome == null) return null;
    final h = await link.readHome();
    if (_link != link) return null;
    final changed = h.mine != nodeHome!.mine || h.set != nodeHome!.set;
    nodeHome = h;
    if (changed) notifyListeners();
    return h;
  }

  /// Writes Home, the mailbox key or none, with a fresh `sent`, and reads
  /// it back.
  Future<NodeHome> writeHome(List<int>? key) async {
    final link = _owned('Home');
    await link.setHome(key == null ? null : await _homeSet(key));
    nodeHome = await link.readHome();
    if (nodeHome!.mine) homeRemoved = false;
    await _readPhones(link);
    note('Home: wrote ${key == null ? 'no key' : 'the key'}, reads $nodeHome');
    notifyListeners();
    return nodeHome!;
  }

  /// Reads the node's neighbour table, page by page (spec "Phone link",
  /// Neighbours); a read asked for while one runs gets that one.
  Future<NeighbourTable> readNeighbours() => _readingNeighbours ??=
      _readNeighbours().whenComplete(() => _readingNeighbours = null);

  /// Reads Status again for its GPS's signal and its age, whose moves the
  /// node does not notify (spec "Phone link", Status); the rest is as notified.
  Future<void> readGps() async {
    final link = _owned('Status');
    final s = await link.readStatus();
    if (_link != link) return;
    status = status?.copyWith(gpsCn0: s.gpsCn0, gpsAge: s.gpsAge) ?? s;
    notifyListeners();
  }

  Future<NeighbourTable> _readNeighbours() async {
    final link = _owned('Neighbours');
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
    notifyListeners();
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

  /// Where `id`'s asks stand (spec "Neighbour status").
  AskState askState(List<int> id) => _asks.state(hex(id));

  /// Asks for its status each of `list` whose asks are not under way and
  /// which has not answered within [Asks.fresh], and asks again each that
  /// has not answered within [Asks.wait] of its ask, up to [Asks.rounds]
  /// asks ([Asks]). An ask and its answer are single frames on a shared
  /// channel and are lost under load, so the one who looks asks again,
  /// not the node. An older answer stays until a new one replaces it.
  Future<void> askNeighbours(Iterable<Neighbour> list) {
    final link = _owned('Ask');
    final ids = _asks.look(list.map((n) => hex(n.id)), now());
    notifyListeners();
    return _send(link, ids);
  }

  /// Writes the asks of `ids` in turn, after those being written, each
  /// one's wait starting as it is written.
  Future<void> _send(NodeLink link, List<String> ids) {
    if (ids.isEmpty) return _asking;
    note('Ask: ${ids.length} neighbours');
    return _asking = _asking.then((_) async {
      for (final id in ids) {
        if (_link != link) return;
        // Its series ended while earlier asks were written: the map shut.
        if (_asks.state(id) != AskState.asking) continue;
        try {
          await link.ask(parseHex(id)!);
        } catch (e) {
          note('Ask: ${nodeName(parseHex(id)!)}: $e');
        }
        if (_link != link) return;
        _asks.sent(id, now());
      }
      notifyListeners();
      _askLater();
    });
  }

  void _askLater() {
    _askTimer?.cancel();
    final at = _asks.next;
    if (at == null) return;
    final wait = at.difference(now());
    _askTimer = Timer(wait.isNegative ? Duration.zero : wait, _askDue);
  }

  void _askDue() {
    final link = _link;
    if (link == null) return;
    final (:again, :gaveUp) = _asks.due(now());
    for (final id in gaveUp) {
      note('Ask: ${nodeName(parseHex(id)!)} did not answer');
    }
    notifyListeners();
    unawaited(_send(link, again));
    if (again.isEmpty) _askLater();
  }

  /// Ends every series of asks: the person stopped looking.
  void stopAsking() {
    _asks.stop();
    _askTimer?.cancel();
    _askTimer = null;
  }

  void _onAnswer(Answer? a) {
    if (a == null) return;
    final key = hex(a.id);
    answers[key] = a.status;
    _asks.answered(key, now());
    note('Ask: ${nodeName(a.id)} answers ${a.status.lines.join(', ')}');
    notifyListeners();
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
      note('Clock: ${describeTime(nodeTime!, english)}');
    } catch (e) {
      note('Clock: $e');
    }
    notifyListeners();
  }

  /// Closes the link, which is detached once this returns: a link's own
  /// report of its end may come later, as Bluetooth's does.
  Future<void> disconnect() async {
    final link = _link;
    if (link == null) return;
    await link.close();
    _detach(link);
  }

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
    _givenBackRead = false;
    nodeTime = null;
    nodeHome = null;
    homeRemoved = false;
    phones = null;
    _phonesAt = null;
    _phonesAgain = false;
    nodePublic = null;
    guest = false;
    neighbours = null;
    answers.clear();
    stopAsking();
    _asks.clear();
    _askedAt = null;
    _askAgain?.cancel();
    note('Disconnected');
    notifyListeners();
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
    final link = _link;
    final at = _phonesAt;
    if (!guest && link != null && at != null && s.phones != at) {
      unawaited(_readPhones(link));
    }
    _neighbourBack(s);
    _pump();
  }

  /// Told of each message given back for no way, once, before the
  /// listeners: a text waits in its thread for the person, and what else
  /// comes back so is said where the person looks for it.
  void Function(Outgoing)? foundNoWay;

  /// Takes the node's Given back. The value stays until the node takes
  /// another message from this phone and is read again at every connect,
  /// so the last one handled from each node is kept ([NodeRecord]) and a
  /// value is new only when it differs.
  void _onGivenBack(GivenBack? g) {
    givenBack = g;
    note(
      g == null
          ? 'Given back: empty'
          : 'Given back: ${msgIdHex(g.msgId)}, ${g.why.name}, '
                'wait ${g.waitS} s',
    );
    final n = node;
    final r = n == null ? null : nodes.of(nodeName(n.id));
    final seen = r?.givenBackSeen;
    r?.givenBackSeen = g?.msgId;
    if (g != null && seen == g.msgId) {
      note('Given back: ${msgIdHex(g.msgId)} read before');
    }
    final o = g == null || seen == g.msgId ? null : _givenBack(g);
    notifyListeners();
    if (seen != g?.msgId || o != null) {
      unawaited(save(out: [?o], state: seen != g?.msgId));
    }
  }

  /// Acts on `g`, read for the first time: the message it names, if the
  /// node had it, given back. Queued counts as had: a write whose answer
  /// was lost was put back in the queue, and the give-back proves the
  /// node took it.
  Outgoing? _givenBack(GivenBack g) {
    final id = msgIdHex(g.msgId);
    final o = outbox.where((o) => o.id == g.msgId).firstOrNull;
    if (o == null) {
      if (outbox.any((o) => o.replaced.contains(g.msgId))) {
        note('Given back: $id was sent again already');
      } else {
        tellIn((w) => w.sessionGivenBackUnknown(id));
      }
      return null;
    }
    if (o.state != OutState.taken && o.state != OutState.queued) {
      note('Given back: $id is ${o.state.name} here');
      return null;
    }
    o.state = OutState.givenBack;
    o.why = g.why;
    switch (g.why) {
      case Why.busy:
        o.retryAt = now().add(Duration(seconds: g.waitS));
        _retryWhenDue();
      case Why.noNeighbour:
        // Status may have said so before this came.
        if (status case final s?) _neighbourBack(s);
      case Why.noWay:
        foundNoWay?.call(o);
    }
    return o;
  }

  /// Offers again each message given back for no neighbour once the node
  /// is ready, or hears a neighbour: ready is the turn, which may be
  /// another phone's until this one asks for it by a write (spec "Phone
  /// link", Sending). A write answered not ready then waits for ready as
  /// any message does.
  void _neighbourBack(Status s) {
    if (!s.ready && s.heard == 0) return;
    final again = [
      for (final o in outbox)
        if (o.state == OutState.givenBack && o.why == Why.noNeighbour) o,
    ];
    if (again.isEmpty) return;
    for (final o in again) {
      o.requeue();
      note('Send ${msgIdHex(o.id)}: the node hears a neighbour, sent again');
    }
    notifyListeners();
    unawaited(save(out: again));
    _pump();
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
        if (o.state == OutState.givenBack && o.why == Why.busy) ?o.retryAt,
    ];
    if (due.isEmpty) return;
    final first = due.reduce((a, b) => a.isBefore(b) ? a : b);
    final wait = first.difference(now());
    _retry = Timer(wait.isNegative ? Duration.zero : wait, () {
      final t = now();
      final again = <Outgoing>[];
      for (final o in outbox) {
        final at = o.retryAt;
        if (o.state == OutState.givenBack &&
            o.why == Why.busy &&
            at != null &&
            !at.isAfter(t)) {
          o.requeue();
          again.add(o);
          note('Send ${msgIdHex(o.id)}: its wait is over, sent again');
        }
      }
      notifyListeners();
      unawaited(save(out: again));
      _pump();
      _retryWhenDue();
    });
  }

  /// Queues a message, saved with the `received` it changed and, with
  /// `state`, the state; the node takes it when ready.
  Future<void> queue(
    Outgoing o, {
    Iterable<Received> received = const [],
    bool state = false,
  }) async {
    outbox.add(o);
    if (o.kind == OutKind.fetch) _fetchOverWhenDue();
    notifyListeners();
    await save(out: [o], received: received, state: state);
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
  /// refused it or given it back for no way, no reply of its round has
  /// come, and [fetchAnswerWait] has not passed since it was queued; one
  /// given back busy or with no neighbour goes again by itself. Stored
  /// with the queue, so it is still waited for after a restart.
  Outgoing? get fetchWaiting {
    final o = outbox.where((o) => o.kind == OutKind.fetch).lastOrNull;
    if (o == null || o.answered) return null;
    if (o.noWay || o.state == OutState.refused) return null;
    return now().difference(o.at) < fetchAnswerWait ? o : null;
  }

  /// Wakes the listeners as a fetch's wait for its answer ends, when it is
  /// no longer [fetchWaiting].
  Timer? _fetchOver;

  void _fetchOverWhenDue() {
    _fetchOver?.cancel();
    final t = now();
    DateTime? first;
    for (final o in outbox) {
      if (o.kind != OutKind.fetch || o.answered) continue;
      final end = o.at.add(fetchAnswerWait);
      if (end.isAfter(t) && (first == null || end.isBefore(first))) {
        first = end;
      }
    }
    if (first == null) return;
    _fetchOver = Timer(first.difference(t), () {
      notifyListeners();
      _fetchOverWhenDue();
    });
  }

  /// Seconds left of the wait for [fetchWaiting]'s answer.
  int fetchWaitLeftS(Outgoing o) {
    final left = fetchAnswerWait - now().difference(o.at);
    return left.isNegative ? 0 : (left.inMilliseconds + 999) ~/ 1000;
  }

  /// What the home node's answer said, to tell the person once.
  String? fetchNews;

  /// A reply of the fetch of `round` came: the fetch is answered. Its
  /// summary, which may come after an item, tells the person what
  /// `remaining` the home holds beyond its batch, so that they can fetch
  /// again.
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
      fetchNews = tr.sessionNoNewMail;
    } else if (remaining > 0 && o != null) {
      note('Fetch $round: $remaining more at home');
      fetchNews = tr.sessionMoreAtHome(remaining);
    }
    if (o == null || o.answered) {
      notifyListeners();
      return;
    }
    o.answered = true;
    notifyListeners();
    await save(out: [o]);
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
    await save(received: [r]);
  }

  Future<void> drop(Outgoing o) async {
    if (o.state == OutState.writing) return;
    outbox.remove(o);
    notifyListeners();
    await _save([
      {'out': _key(o)},
    ]);
  }

  /// Sends the text whose parts are `old`, one given back for no way, again
  /// as a new text, the parts `make` seals given the IDs it replaces: the
  /// old parts leave the queue at once, before anything waits, so none of
  /// them is offered to the node again and a second ask finds nothing;
  /// they come back if `make` fails. Whether it was sent again.
  Future<bool> sendAgain(
    List<Outgoing> old,
    Future<List<Outgoing>> Function(List<int> replaced) make,
  ) async {
    if (!old.any((o) => o.noWay) ||
        old.any((o) => o.state == OutState.writing) ||
        !old.every(outbox.contains)) {
      return false;
    }
    final places = [for (final o in old) outbox.indexOf(o)]..sort();
    final kept = [for (final i in places) outbox[i]];
    outbox.removeWhere(old.contains);
    notifyListeners();
    final List<Outgoing> parts;
    try {
      parts = await make([
        for (final o in old) ...[o.id, ...o.replaced],
      ]);
    } catch (_) {
      for (final (i, at) in places.indexed) {
        outbox.insert(at, kept[i]);
      }
      notifyListeners();
      rethrow;
    }
    outbox.addAll(parts);
    notifyListeners();
    await _save([
      for (final o in old) {'out': _key(o)},
      for (final o in parts) _outChange(o),
    ]);
    _pump();
    return true;
  }

  /// How long the app waits for Ready after asking for the turn before it
  /// asks again (spec "Phone link", Sending): a ban, 1.58 s, for each
  /// other of the node's [maxPhones] + 1 links, the longest the turn takes
  /// to reach it while the node takes messages.
  static const turnWait = Duration(milliseconds: maxPhones * 1580);

  /// When the app last asked for the turn on this link, by a write
  /// answered not ready.
  DateTime? _askedAt;
  Timer? _askAgain;

  Future<void> _pump() async {
    final link = _link;
    final n = node;
    if (_writing || link == null || n == null || !_givenBackRead) return;
    final o = outbox.where((o) => o.state == OutState.queued).firstOrNull;
    if (o == null) return;
    // Out of turn, the write asks for it, unless it asked lately.
    final ready = status?.ready ?? false;
    if (!ready) {
      final asked = _askedAt;
      if (asked != null && now().difference(asked) < turnWait) return;
    }
    _writing = true;
    o.state = OutState.writing;
    notifyListeners();
    var again = true;
    var seenCleared = false;
    try {
      await link.send(encodeSend(o.dst, o.payload));
      _askedAt = null;
      o.state = OutState.taken;
      note('Send ${msgIdHex(o.id)}: taken');
      // The node's Given back for this phone is empty from this take on.
      final r = nodes.of(nodeName(n.id));
      seenCleared = r.givenBackSeen != null;
      r.givenBackSeen = null;
    } on AttException catch (e) {
      note('Send ${msgIdHex(o.id)}: $e');
      if (e.code == AttError.notReady) {
        // Not this phone's turn, which the write asked for, or Ready
        // turned false between the status read and the write; wait for
        // the node to say ready again, and read it in case that
        // notification came before this answer.
        o.state = OutState.queued;
        status = status?.copyWith(ready: false);
        again = false;
        _askedAt = now();
        _askAgain?.cancel();
        _askAgain = Timer(turnWait, _pump);
        unawaited(
          link.readStatus().then((s) {
            if (_link == link) _onStatus(s);
          }, onError: (_) {}),
        );
      } else {
        o.state = OutState.refused;
        o.error = '$e';
      }
    } catch (e) {
      o.state = OutState.queued;
      again = false;
      tellIn((w) => w.sendFailed(said(e, w)));
    } finally {
      _writing = false;
    }
    notifyListeners();
    await save(out: [o], state: seenCleared);
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
          tellIn((w) => w.sessionConfirmRefused(said(e, w)));
          break;
        }
      }
    } catch (e) {
      tellIn((w) => w.sessionInboxFailed(said(e, w)));
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
      tellIn((w) => w.sessionPlaceFailed(said(e, w)));
    }
  }

  @override
  void dispose() {
    _retry?.cancel();
    _fetchOver?.cancel();
    _askAgain?.cancel();
    for (final s in _subs) {
      s.cancel();
    }
    _statusSeen.close();
    super.dispose();
  }
}

/// The node's time as a person reads it: UTC, the source, the offset; in
/// `words`, by default the language spoken.
String describeTime(NodeTime t, [AppLocalizations? words]) {
  final w = words ?? tr;
  final o = t.offsetMin;
  final sign = o < 0 ? '-' : '+';
  final offset =
      'UTC$sign${(o.abs() ~/ 60).toString().padLeft(2, '0')}:'
      '${(o.abs() % 60).toString().padLeft(2, '0')}';
  final utc = DateTime.fromMillisecondsSinceEpoch(
    t.utcS * 1000,
    isUtc: true,
  ).toIso8601String();
  return switch (t.source) {
    TimeSource.none => w.sessionClockNone(offset),
    TimeSource.gps => w.sessionClockGps(utc, offset),
    TimeSource.phone => w.sessionClockPhone(utc, offset),
  };
}

/// Why the node was not asked for something only its paired phone may
/// ask: said to the person as it is, after what failed.
class NotOwned extends StateError implements Spoken {
  final String Function(AppLocalizations) _why;
  NotOwned(String Function(AppLocalizations) why)
    : _why = why,
      super(why(english));

  @override
  String words(AppLocalizations w) => _why(w);
  @override
  String toString() => message;
}
