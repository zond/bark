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

  /// When the home was set or cleared, as `sent`.
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

  Uint8List get identity => keys.identity;
  String get key => hex(identity);
  List<String> get phrase => phraseOf(keys.seed);

  PersonCard get card => PersonCard(identity, home, cardTime, name);

  /// The version of `me.json`'s format.
  static const version = 1;

  Future<void> load() async {
    final apply = await store.load(version, (j) {
      final keys = Keys.fromSeed(sodium, parseHex(j['seed'] as String)!);
      final name = j['name'] as String;
      final home = _address(j['home']);
      final nodeAddress = _address(j['nodeAddress']);
      final cardTime = j['cardTime'] as int;
      final lastSent = j['lastSent'] as int;
      final backedUp = j['backedUp'] as bool;
      final since = j['since'] as int;
      final above = (j['above'] as List).cast<int>().toList();
      final rounds = (j['rounds'] as List).cast<int>().toList();
      return () {
        this.keys = keys;
        this.name = name;
        this.home = home;
        this.nodeAddress = nodeAddress;
        this.cardTime = cardTime;
        this.lastSent = lastSent;
        this.backedUp = backedUp;
        this.since = since;
        this.above.addAll(above);
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

  /// Sets or clears the home node; true if the card changed.
  Future<bool> setHome(Address? a) async {
    final changed = a?.toString() != home?.toString();
    home = a;
    if (changed) cardTime = sentOf(now());
    await _changed();
    return changed;
  }

  /// The connected node's address: where the person writes from.
  Future<void> setNodeAddress(Address a) async {
    if (a.toString() == nodeAddress?.toString()) return;
    nodeAddress = a;
    await _changed();
  }

  /// A fetch of round `round` was sent.
  Future<void> fetched(int round) async {
    rounds.insert(0, round);
    if (rounds.length > keptRounds) rounds.removeLast();
    await _changed();
  }

  /// Item `n` of the mailbox has arrived, in a round whose first item was
  /// `first`: the ring held nothing after the fetch's `since` below that,
  /// and never will.
  Future<void> gotItem(int n, {required int first}) async {
    if (first - 1 > since) {
      since = first - 1;
      above.removeWhere((a) => a <= since);
    }
    if (n > since) above.add(n);
    while (above.remove(since + 1)) {
      since++;
    }
    await _changed();
  }
}
