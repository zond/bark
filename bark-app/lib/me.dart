// The person: their identity, made at first start from a seed they can
// back up as 12 words, their name, their home node, the numbers their
// texts take, and what their mailbox has sent them.

import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';

import 'app_layer.dart';
import 'card.dart';
import 'identity_file.dart';
import 'link.dart';
import 'store.dart';

class Me extends ChangeNotifier {
  final Store store;
  final Sodium sodium;
  final DateTime Function() now;
  Me(this.store, this.sodium, this.now);

  late Keys keys;

  /// The name on the person's card.
  String name = '';

  /// The node the person set as home, whose address their card carries;
  /// without one the card carries no address.
  Address? home;

  /// The address of the node the phone is connected to, as last read.
  Address? nodeAddress;

  /// When the phone was last linked to [nodeAddress], as `sent`, set as
  /// the link begins and ends.
  int nodeAt = 0;

  /// The phone is linked to [nodeAddress] now.
  bool linked = false;

  /// When the home was set or cleared, as `sent`.
  int cardTime = 0;

  /// The `cardTime` of the card last sent to every contact.
  int cardSentTime = 0;

  /// The card changed since it was last sent to every contact.
  bool get cardUnsent => cardTime > cardSentTime;

  /// The last `sent` this phone gave a message.
  int lastSent = 0;

  /// The person has seen their backup words.
  bool backedUp = false;

  /// The app has asked the person to let it notify.
  bool askedToNotify = false;

  /// The app has asked the person to let it use Bluetooth.
  bool askedBluetooth = false;

  /// A fetch the person asked for while no node was linked: it goes out
  /// from the next node the phone links to.
  bool fetchQueued = false;

  /// The mailbox: the items its home last said it has not sent, and the
  /// rounds of its latest fetches, newest first.
  int remaining = 0;
  final List<int> rounds = [];

  /// The mailbox key the home holds when it is someone else's, who gave
  /// it to the person with the home; null for the person's own, made of
  /// their identity.
  Uint8List? givenKey;
  SecureKey? _given;

  /// The key the home holds, which seals the person's fetches to it.
  SecureKey get mailboxKey {
    final g = givenKey;
    if (g == null) return keys.mailbox;
    return _given ??= SecureKey.fromList(sodium, g);
  }

  /// The home says the person's key is its owner's, as last told: by a
  /// summary, or by Phones at the home (spec "Phone link", The node's
  /// owner).
  bool owner = false;

  /// The home the person is leaving, until its mail is fetched and it is
  /// told to forget them.
  Leaving? leaving;

  /// The rounds whose replies are still opened: a reply of the round
  /// before may still be on its way.
  static const keptRounds = 2;

  Uint8List get identity => keys.identity;
  String get key => hex(identity);
  List<String> get phrase => phraseOf(keys.seed);

  /// The card, with the node the phone is linked to, or was last.
  PersonCard get card {
    final n = nodeAddress;
    final atHome = n != null && n.toString() == home?.toString();
    return PersonCard(
      identity,
      home,
      cardTime,
      name,
      now: atHome ? null : n,
      // While linked, now, to the minute, the span within which a sender
      // takes a place as fresh (`Contact.fresh`), so that the code shown
      // does not change every second.
      nowAt: linked ? sentOf(now()) ~/ 60 * 60 : nodeAt,
    );
  }

  /// The card to show or share: none until the person has a name, as a
  /// contact made from a nameless card is one its holder cannot tell
  /// apart.
  PersonCard? get shownCard => name.isEmpty ? null : card;

  /// The version of `me.json`'s format.
  static const version = 1;

  Future<void> load() async {
    final apply = await store.load(version, (j) {
      final keys = Keys.fromSeed(sodium, parseHex(j['seed'] as String)!);
      final name = j['name'] as String;
      final home = _address(j['home']);
      final nodeAddress = _address(j['nodeAddress']);
      final nodeAt = j['nodeAt'] as int;
      final cardTime = j['cardTime'] as int;
      final cardSentTime = j['cardSentTime'] as int;
      final lastSent = j['lastSent'] as int;
      final backedUp = j['backedUp'] as bool;
      final askedToNotify = j['askedToNotify'] as bool;
      final askedBluetooth = j['askedBluetooth'] as bool;
      final fetchQueued = j['fetchQueued'] as bool;
      final remaining = j['remaining'] as int;
      final rounds = (j['rounds'] as List).cast<int>().toList();
      // A file without these has the person's own home, if any, of which
      // they are not known as the owner, and leaves none.
      final givenKey = switch (j['homeKey']) {
        null => null,
        final k => parseHex(k as String)!,
      };
      final owner = j['owner'] as bool? ?? false;
      final leaving = switch (j['leaving']) {
        null => null,
        final l => Leaving.fromJson(l as Map<String, dynamic>),
      };
      return () {
        this.givenKey = givenKey;
        this.owner = owner;
        this.leaving = leaving;
        this.keys = keys;
        this.name = name;
        this.home = home;
        this.nodeAddress = nodeAddress;
        this.nodeAt = nodeAt;
        this.cardTime = cardTime;
        this.cardSentTime = cardSentTime;
        this.lastSent = lastSent;
        this.backedUp = backedUp;
        this.askedToNotify = askedToNotify;
        this.askedBluetooth = askedBluetooth;
        this.fetchQueued = fetchQueued;
        this.remaining = remaining;
        this.rounds.addAll(rounds);
      };
    });
    if (apply == null) {
      keys = Keys.fromSeed(sodium, sodium.randombytes.buf(seedLen));
      await _save();
    } else {
      apply();
    }
    notifyListeners();
  }

  static Address? _address(Object? s) =>
      s == null ? null : Address.parse(s as String);

  Future<void> _save() => store.save(version, {
    'seed': hex(keys.seed),
    'name': name,
    'home': home?.toString(),
    'nodeAddress': nodeAddress?.toString(),
    'nodeAt': nodeAt,
    'cardTime': cardTime,
    'cardSentTime': cardSentTime,
    'lastSent': lastSent,
    'backedUp': backedUp,
    'askedToNotify': askedToNotify,
    'askedBluetooth': askedBluetooth,
    'fetchQueued': fetchQueued,
    'remaining': remaining,
    'rounds': rounds,
    if (givenKey case final k?) 'homeKey': hex(k),
    'owner': owner,
    if (leaving case final l?) 'leaving': l.toJson(),
  });

  Future<void> _changed() async {
    notifyListeners();
    await _save();
  }

  /// The next `sent`: the clock, or one past the last if the clock is
  /// behind it.
  Future<int> nextSent() => nextSents(1);

  /// The first of `n` consecutive `sent`, taken at once, as a split
  /// text's parts are named by them.
  Future<int> nextSents(int n) async {
    final first = max(sentOf(now()), lastSent + 1);
    lastSent = first + n - 1;
    await _save();
    return first;
  }

  Future<void> setName(String n) async {
    name = n.trim();
    await _changed();
  }

  Future<void> setBackedUp() async {
    backedUp = true;
    await _changed();
  }

  Future<void> setAskedToNotify() async {
    askedToNotify = true;
    await _changed();
  }

  Future<void> setAskedBluetooth() async {
    askedBluetooth = true;
    await _changed();
  }

  Future<void> setFetchQueued(bool v) async {
    if (fetchQueued == v) return;
    fetchQueued = v;
    await _changed();
  }

  /// Replaces the identity by the one of `seed`, from its 12 words.
  Future<void> restore(Uint8List seed) async {
    keys = Keys.fromSeed(sodium, seed);
    backedUp = true;
    remaining = 0;
    rounds.clear();
    await _changed();
  }

  /// Takes the identity and what goes with it from an identity file; what
  /// this phone has asked of Android stays its own. Applied at once, saved
  /// after.
  Future<void> moveIn(IdentityFile f) {
    keys = Keys.fromSeed(sodium, f.seed);
    name = f.name;
    home = f.home?.address;
    final k = f.home?.key;
    _givenKey(
      k == null || hex(k) == hex(keys.mailbox.extractBytes()) ? null : k,
    );
    // The next summary says it again.
    owner = false;
    leaving = null;
    nodeAddress = f.nodeAddress;
    nodeAt = f.nodeAt;
    linked = false;
    cardTime = f.cardTime;
    cardSentTime = f.cardSentTime;
    lastSent = f.lastSent;
    backedUp = f.backedUp;
    fetchQueued = false;
    remaining = f.remaining;
    rounds
      ..clear()
      ..addAll(f.rounds);
    notifyListeners();
    return _save();
  }

  void _givenKey(Uint8List? k) {
    _given?.dispose();
    _given = null;
    givenKey = k;
  }

  /// Sets or clears the home node, with the `key` its owner gave the
  /// person if it is someone else's, none for their own: the same node at
  /// a new place keeps its key and owner. True if the card changed.
  Future<bool> setHome(Address? a, {Uint8List? key}) async {
    final changed = a?.toString() != home?.toString();
    if (a == null || home == null || hex(a.id) != hex(home!.id)) {
      _givenKey(key);
      owner = false;
    }
    home = a;
    // One past the last, so that a second change within a second is still
    // a newer card.
    if (changed) cardTime = max(sentOf(now()), cardTime + 1);
    await _changed();
    return changed;
  }

  /// The card of time `t` went to every contact.
  Future<void> setCardSent(int t) async {
    cardSentTime = max(cardSentTime, t);
    await _changed();
  }

  /// The connected node's address: where the person writes from.
  Future<void> setNodeAddress(Address a) async {
    nodeAddress = a;
    nodeAt = sentOf(now());
    linked = true;
    await _changed();
  }

  /// The link to the node ended now.
  Future<void> unlinked() async {
    linked = false;
    nodeAt = sentOf(now());
    await _changed();
  }

  /// A fetch of round `round` was sent.
  Future<void> fetched(int round) async {
    rounds.insert(0, round);
    if (rounds.length > keptRounds) rounds.removeLast();
    await _changed();
  }

  /// The summary of the fetch of `round` has come: if it is the latest
  /// fetch's, its count of the items the home has not sent, and whether
  /// the person is its owner, are current.
  Future<void> gotSummary(int round, RoundSummary o) async {
    if (rounds.isEmpty || round != rounds.first) return;
    remaining = o.remaining;
    owner = o.owner;
    await _changed();
  }

  /// Whether the home holds the person's key as its owner's, as the home
  /// said at the node or the app learnt otherwise.
  Future<void> setOwner(bool o) async {
    if (owner == o) return;
    owner = o;
    await _changed();
  }

  /// Starts leaving `l`, or ends it with null.
  Future<void> setLeaving(Leaving? l) async {
    leaving = l;
    await _changed();
  }

  /// The leaving's state changed.
  Future<void> leavingChanged() => _changed();
}

/// A home the person is leaving: its address and the key it holds. The
/// app fetches the mail it keeps, then tells it to forget the person
/// (spec "Mailbox", The owner's commands), as a home left behind would
/// keep their mail where nobody fetches it.
class Leaving {
  final Address home;
  final Uint8List key;

  /// The latest fetch from it, as `sent`, and when it was queued; null
  /// before the first.
  int? round;
  DateTime? at;

  /// What that fetch's summary said, once it came: the items it carries
  /// and those still kept after them; and the items of it opened so far.
  int? items;
  int remaining = 0;
  int got = 0;

  /// The leave once sent, as `sent`, and when it was queued.
  int? leaveRound;
  DateTime? leaveAt;

  Leaving(this.home, List<int> key) : key = Uint8List.fromList(key);

  Map<String, dynamic> toJson() => {
    'home': home.toString(),
    'key': hex(key),
    'round': round,
    'at': at?.toIso8601String(),
    'items': items,
    'remaining': remaining,
    'got': got,
    'leaveRound': leaveRound,
    'leaveAt': leaveAt?.toIso8601String(),
  };

  static Leaving fromJson(Map<String, dynamic> j) {
    DateTime? time(Object? t) => t == null ? null : DateTime.parse(t as String);
    return Leaving(
        Address.parse(j['home'] as String)!,
        parseHex(j['key'] as String)!,
      )
      ..round = j['round'] as int?
      ..at = time(j['at'])
      ..items = j['items'] as int?
      ..remaining = j['remaining'] as int
      ..got = j['got'] as int
      ..leaveRound = j['leaveRound'] as int?
      ..leaveAt = time(j['leaveAt']);
  }
}
