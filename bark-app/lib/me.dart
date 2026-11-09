// The person: their identity, made at first start from a seed they can
// back up as 12 words, their name, their home node, the numbers their
// texts take, and what their mailbox has sent them.

import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';

import 'app_layer.dart';
import 'card.dart';
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

  /// The person has tapped Got it once, so the hint is no longer needed.
  bool gotItHinted = false;

  /// A fetch the person asked for while no node was linked: it goes out
  /// from the next node the phone links to.
  bool fetchQueued = false;

  /// The mailbox: the items its home last said it has not sent, and the
  /// rounds of its latest fetches, newest first.
  int remaining = 0;
  final List<int> rounds = [];

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
      // Added with D162: absent, the link's time is not known.
      final nodeAt = j['nodeAt'] as int? ?? 0;
      final cardTime = j['cardTime'] as int;
      // Added after the format's first version: before it, every changed
      // card was sent at once, so absent means sent.
      final cardSentTime = j['cardSentTime'] as int? ?? cardTime;
      final lastSent = j['lastSent'] as int;
      final backedUp = j['backedUp'] as bool;
      // Added after the format's first version: absent means not asked.
      final askedToNotify = j['askedToNotify'] as bool? ?? false;
      // Added after the format's first version: absent means not asked,
      // none queued.
      final askedBluetooth = j['askedBluetooth'] as bool? ?? false;
      final fetchQueued = j['fetchQueued'] as bool? ?? false;
      final gotItHinted = j['gotItHinted'] as bool? ?? false;
      // Added after the format's first version: absent means none told.
      final remaining = j['remaining'] as int? ?? 0;
      final rounds = (j['rounds'] as List).cast<int>().toList();
      return () {
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
        this.gotItHinted = gotItHinted;
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
    'gotItHinted': gotItHinted,
    'remaining': remaining,
    'rounds': rounds,
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

  Future<void> setGotItHinted() async {
    if (gotItHinted) return;
    gotItHinted = true;
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

  /// Sets or clears the home node; true if the card changed.
  Future<bool> setHome(Address? a) async {
    final changed = a?.toString() != home?.toString();
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

  /// A reply of the fetch of `round` has come: if it is the latest
  /// fetch's, its count of the items the home has not sent is current.
  Future<void> gotReply(int round, Opened o) async {
    if (rounds.isEmpty || round != rounds.first) return;
    remaining = o.remaining;
    await _changed();
  }
}
