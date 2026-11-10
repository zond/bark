// Pairing and Android's bonds: unpairing, forgetting a node no longer
// the phone's, pairing with one again, and the bonds Android holds with
// nodes that are not the person's, which go (roadmap D194, D203).

part of '../app.dart';

mixin _Pairing on _AppCore {
  /// After a connect found the node no longer this phone's: forgets the
  /// old bond and connects again, which pairs.
  @override
  Future<void> Function()? pairAgain;

  /// A node whose bond Android kept ([NodeRecord.bondKept]), for the line
  /// that tells the person.
  String? get bondKept =>
      nodes.records.where((r) => r.bondKept).firstOrNull?.name;

  /// Clears [NodeRecord.bondKept] of each node whose bond Android no
  /// longer lists: the person has forgotten it in the phone's settings.
  @override
  Future<void> recheckKept() async {
    for (final r in [...nodes.records.where((r) => r.bondKept)]) {
      final device = r.heard?.deviceId;
      if (device == null || await BleLink.bonded(device)) continue;
      r.bondKept = false;
      session.note('Pairing: the phone no longer holds a bond with ${r.name}');
      notifyListeners();
    }
  }

  /// Has Android remove its bond with the node `name` at `device`, if it
  /// lists one, as the app makes no link over a bond to a node it does
  /// not count as its own: the node would take the link as its owner's,
  /// or end it (spec "Phone link", Public). True once none is left; where
  /// Android keeps it, false, and the node's record says so
  /// ([NodeRecord.bondKept]).
  @override
  Future<bool> _forgetBond(String name, String device) async {
    if (!await BleLink.bonded(device)) return true;
    final r = nodes.of(name);
    if (await BleLink.forget(device)) {
      r.unseen = true;
      session.note('Pairing: the phone forgot its bond with $name');
      return true;
    }
    if (!r.bondKept) {
      session.note('Pairing: the phone kept its bond with $name');
    }
    r.bondKept = true;
    notifyListeners();
    return false;
  }

  /// Removes the bond Android holds with each node of `heard` that the app
  /// does not count as its own, as it hears it, so that the node is a
  /// public node like any other.
  @override
  Future<void> _forgetBonds(Iterable<FoundNode> heard) async {
    for (final n in heard) {
      if (n.name == n.deviceId) continue;
      final r = nodes.of(n.name);
      if (r.own || r.bondKept) continue;
      await _forgetBond(n.name, n.deviceId);
    }
  }

  /// The owner gives the connected node up: the node forgets this phone's
  /// bond, then the phone forgets its own, as Android would encrypt every
  /// later link to the node with it, a guest's too, and the node refuses
  /// it (spec "Phone link", Public). Returns whether the phone's bond is
  /// gone; where Android keeps it, the person forgets the node in the
  /// phone's Bluetooth settings, and [bondKept] says so meanwhile.
  Future<bool> unpair() async {
    _unpairing = true;
    notifyListeners();
    try {
      return await _unpair();
    } finally {
      _unpairing = false;
      notifyListeners();
    }
  }

  /// An unpairing is under way: the search waits, as the node ends the
  /// link while the phone still holds the bond, and a connect with it
  /// then would find the node no longer this phone's.
  @override
  bool _unpairing = false;

  Future<bool> _unpair() async {
    final device = linkDevice;
    final n = session.node;
    await session.unpair();
    if (device == null || n == null) return true;
    final name = nodeName(n.id);
    _roundsAfresh();
    return _forgetBond(name, device);
  }

  /// The node `name` as last heard, if heard within the span the search
  /// takes to call a node out of reach ([ownGone] rounds): in reach.
  @override
  FoundNode? heardLately(String name) {
    final r = nodes[name];
    final at = r?.heardAt;
    return at != null && now().difference(at) < lookRound * ownGone
        ? r!.heard
        : null;
  }

  /// Whether `name`, a paired node no longer this phone's, may be paired
  /// with again now ([pairLost]): heard lately.
  bool lostInReach(String name) => heardLately(name) != null;

  /// The person pairs again with `name`, a paired node that no longer
  /// knows this phone, by the code on its screen, as **Pair** on its row
  /// in a scan does ([connect]).
  Future<void> pairLost(String name) async {
    final n = heardLately(name);
    if (n == null) throw NotSeen(shownName(name));
    await connect(n);
  }

  /// Forgets `name`, a paired node no longer this phone's: the app counts
  /// it paired no more, and has Android remove its bond with the node, as
  /// [unpair] does, where it has heard the node in this run (roadmap
  /// D173). Returns true once no bond is left, false where Android kept
  /// it ([bondKept] then says so), and null where the app has not heard
  /// the node: it removes the bond once it does ([_forgetBonds]), or the
  /// person forgets the node in the phone's Bluetooth settings. The home
  /// stays, as the node keeps the mailbox key.
  Future<bool?> forget(String name) async {
    await session.forget(name);
    final device = nodes[name]?.heard?.deviceId;
    if (device == null) return null;
    return _forgetBond(name, device);
  }
}
