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

  /// The node the person set as home, whose address their card carries.
  Address? home;

  /// The address of the node the phone is connected to, as last read.
  Address? nodeAddress;

  /// When the card's address became the person's, as `sent`.
  int cardTime = 0;

  /// The last `sent` this phone gave a message.
  int lastSent = 0;

  /// The person has seen their backup words.
  bool backedUp = false;

  /// The mailbox: the number up to which this phone has every item, the
  /// numbers it has above that, and the rounds of its latest fetches,
  /// newest first.
  int since = 0;
  final Set<int> above = {};
  final List<int> rounds = [];

  /// The rounds whose replies are still opened: a reply of the round
  /// before may still be on its way.
  static const keptRounds = 2;

  /// The most items a fetch asks for: the node's inbox, so that a reply
  /// never overruns it (spec "Mailbox").
  static const fetchMost = 23;

  Uint8List get identity => keys.identity;
  String get key => hex(identity);
  List<String> get phrase => phraseOf(keys.seed);

  /// Where others reach the person: their home, or their node now.
  Address? get cardAddress => home ?? nodeAddress;

  PersonCard? get card {
    final a = cardAddress;
    return a == null ? null : PersonCard(identity, a, cardTime, name);
  }

  Future<void> load() async {
    final j = await store.load();
    if (j == null) {
      keys = Keys.fromSeed(sodium, sodium.randombytes.buf(seedLen));
      await _save();
      notifyListeners();
      return;
    }
    keys = Keys.fromSeed(sodium, parseHex(j['seed'] as String)!);
    name = j['name'] as String;
    home = _address(j['home']);
    nodeAddress = _address(j['nodeAddress']);
    cardTime = j['cardTime'] as int;
    lastSent = j['lastSent'] as int;
    backedUp = j['backedUp'] as bool;
    since = j['since'] as int;
    above.addAll((j['above'] as List).cast<int>());
    rounds.addAll((j['rounds'] as List).cast<int>());
    notifyListeners();
  }

  static Address? _address(Object? s) =>
      s == null ? null : Address.parse(s as String);

  Future<void> _save() => store.save({
    'seed': hex(keys.seed),
    'name': name,
    'home': home?.toString(),
    'nodeAddress': nodeAddress?.toString(),
    'cardTime': cardTime,
    'lastSent': lastSent,
    'backedUp': backedUp,
    'since': since,
    'above': above.toList(),
    'rounds': rounds,
  });

  Future<void> _changed() async {
    notifyListeners();
    await _save();
  }

  /// The next `sent`: the clock, or one past the last if the clock is
  /// behind it.
  Future<int> nextSent() async {
    lastSent = max(sentOf(now()), lastSent + 1);
    await _save();
    return lastSent;
  }

  Future<void> setName(String n) async {
    name = n;
    await _changed();
  }

  Future<void> setBackedUp() async {
    backedUp = true;
    await _changed();
  }

  /// Replaces the identity by the one of `seed`, from its 12 words.
  Future<void> restore(Uint8List seed) async {
    keys = Keys.fromSeed(sodium, seed);
    backedUp = true;
    since = 0;
    above.clear();
    rounds.clear();
    await _changed();
  }

  /// Sets or clears the home node; true if the card's address changed.
  Future<bool> setHome(Address? a) async {
    final before = cardAddress?.toString();
    home = a;
    return _addressChanged(before);
  }

  /// The connected node's address; true if the card's address changed.
  Future<bool> setNodeAddress(Address a) async {
    if (a.toString() == nodeAddress?.toString()) return false;
    final before = cardAddress?.toString();
    nodeAddress = a;
    return _addressChanged(before);
  }

  Future<bool> _addressChanged(String? before) async {
    final changed = cardAddress?.toString() != before;
    if (changed) cardTime = sentOf(now());
    await _changed();
    return changed;
  }

  /// A fetch of round `round` was sent.
  Future<void> fetched(int round) async {
    rounds.insert(0, round);
    if (rounds.length > keptRounds) rounds.removeLast();
    await _changed();
  }

  /// Item `n` of the mailbox has arrived.
  Future<void> gotItem(int n) async {
    if (n > since) above.add(n);
    while (above.remove(since + 1)) {
      since++;
    }
    await _changed();
  }
}
