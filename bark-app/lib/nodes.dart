// What the phone knows of each node, one record a node, by its name: the
// only place it is kept. Whether the phone is paired with the node, and
// where it stands in the node's ring as a guest, outlive the app's run;
// the rest is learnt again from scans and links.

import 'ble_link.dart';
import 'link.dart';

/// What a guest's errand at a public node read of it, kept between
/// errands, as the link is down meanwhile: its address once it has a
/// place, whether it can be a home ([canBeHome]), and how many more
/// phones may pair with it, `room`, 0 on a node that would refuse a
/// pairing (spec "Phone link", Pairing).
typedef GuestReading = ({Address? address, bool homeable, int? room});

/// Whether a node, by its status, can be a home: one that stays on power
/// at a fixed place, so that its address stays (spec "Mailbox").
bool canBeHome(Status? s) => s != null && !s.sleepy && !s.carried;

class NodeRecord {
  final String name;
  NodeRecord(this.name);

  /// The phone is paired with the node. The app's list of paired nodes
  /// is the only word on pairing: a bond Android holds with a node not
  /// on it the app removes (roadmap D203).
  bool paired = false;

  /// Paired, but the node no longer holds the phone's bond: a phone
  /// paired with it removed this one, or it lost its memory (spec "Phone
  /// link", The removed phone). The person decides, with Pair again or
  /// Forget.
  bool lost = false;

  /// The number of the last item taken from its ring as a guest: the
  /// node keeps nothing for a guest, so the phone keeps where it is
  /// (spec "Phone link", Public).
  int ringAfter = 0;

  /// The node as a scan last heard it, by its device and signal, and
  /// when. Android names a bond by the device, which the app learns only
  /// by hearing the node.
  FoundNode? heard;
  DateTime? heardAt;

  /// Android kept its bond with the node when asked to remove it, the
  /// node not being the phone's: every link to it would be encrypted with
  /// that bond, which the node ends, so it is not used until the person
  /// forgets it in the phone's Bluetooth settings ([BondKept]).
  bool bondKept = false;

  /// Android forgot, with the bond it removed, what kind of address the
  /// node has, and connects to it only once a scan has heard it again
  /// ([seen]).
  bool unseen = false;

  /// When a guest's errand last read its ring through, and what the last
  /// errand there read of it.
  DateTime? ringReadAt;
  GuestReading? asGuest;

  /// Paired, and the node still holds the phone's bond: the phone is its
  /// owner.
  bool get own => paired && !lost;

  /// What of it outlives the app's run.
  bool get _kept => paired || ringAfter != 0;

  Map<String, dynamic> toJson() => {
    'name': name,
    'paired': paired,
    'lost': lost,
    'ringAfter': ringAfter,
  };

  static NodeRecord fromJson(Map<String, dynamic> j) =>
      NodeRecord(j['name'] as String)
        ..paired = j['paired'] as bool
        ..lost = j['lost'] as bool
        ..ringAfter = j['ringAfter'] as int;
}

/// The records, by name, a paired node's moved to the end each time the
/// phone links to it as its owner, so that the one linked to last is
/// last.
class NodeStore {
  final Map<String, NodeRecord> _all = {};

  /// The record of `name`, made empty if there is none.
  NodeRecord of(String name) => _all.putIfAbsent(name, () => NodeRecord(name));

  NodeRecord? operator [](String name) => _all[name];

  Iterable<NodeRecord> get records => _all.values;

  /// The nodes the phone is paired with, the one linked to last last.
  Set<String> get paired => {
    for (final r in _all.values)
      if (r.paired) r.name,
  };

  /// The paired nodes that no longer hold the phone's bond.
  Set<String> get lost => {
    for (final r in _all.values)
      if (r.paired && r.lost) r.name,
  };

  /// The paired nodes that still hold it: the phone is their owner.
  Set<String> get own => {
    for (final r in _all.values)
      if (r.own) r.name,
  };

  /// The public node the phone uses as a guest between the search's
  /// errands, while no round has found none and the phone has not reached
  /// a node of its own since.
  String? guest;

  /// Takes `n` as heard now; a node heard before its name came is known by
  /// its device alone, and is not recorded.
  void heard(FoundNode n, DateTime at) {
    if (n.name == n.deviceId) return;
    final r = of(n.name);
    r.heard = n;
    r.heardAt = at;
    r.unseen = false;
  }

  /// The node `name` holds the phone's bond, or no longer does: moved to
  /// the end of the paired. Whether anything changed.
  bool setPaired(String name, {bool lost = false}) {
    final same = paired.lastOrNull == name && _all[name]!.lost == lost;
    final r = _all.remove(name) ?? NodeRecord(name);
    _all[name] = r
      ..paired = true
      ..lost = lost;
    return !same;
  }

  /// The phone is paired with `name` no more. Whether it was.
  bool unpaired(String name) {
    final r = _all[name];
    if (r == null || !r.paired) return false;
    r
      ..paired = false
      ..lost = false;
    return true;
  }

  List<Map<String, dynamic>> toJson() => [
    for (final r in _all.values)
      if (r._kept) r.toJson(),
  ];

  /// Takes the records read from the file, in their order.
  void load(Iterable<NodeRecord> rs) {
    for (final r in rs) {
      _all[r.name] = r;
    }
  }
}
