// Whom the map asks for its status, and when (spec "Neighbour status",
// "Phone link", Ask). One rule serves the map's opening, its refresh and
// a tap on a neighbour: a node is asked unless its asks are under way or
// it answered lately. Each ask is one frame on the air, two with the
// answer, to a sleepy node with the long preamble, so a node never has
// two series of asks at once, and taps in quick succession add none.

/// Where a node's asks stand.
enum AskState {
  /// Not asked while the map has been open.
  never,

  /// Asked, and waited for; asked again while asks are left.
  asking,

  /// It answered.
  answered,

  /// Its asks ran out with no answer.
  gaveUp,
}

class _Ask {
  AskState state;

  /// While asking, the asks left after the one under way.
  int left;

  /// While asking, when it is asked again, null until its ask is written;
  /// otherwise when it answered or was given up.
  DateTime? at;
  _Ask(this.state, {this.left = 0, this.at});
}

/// Each node's asks, by its ID in hex. Holds no clock or timer: the
/// session gives the time and asks whom it is told to.
class Asks {
  /// Asks of a node in one series, the first and two more.
  static const rounds = 3;

  /// How long after its ask a node is waited for before it is asked again.
  /// On a quiet channel an answer comes about 100 ms after the ask (the
  /// bench, 2026-11-09); under load each of the two frames may wait two
  /// backoffs of up to 200 ms and three checks of 3.1 ms (spec
  /// "Heartbeat"), and to a sleepy node they carry the long preamble,
  /// 249.92 and 257.60 ms on air (spec "Neighbour status"): 2 × 409.3 ms +
  /// 507.52 ms = 1.33 s at most more. 2 s leaves the phone link and the
  /// node's other frames room.
  static const wait = Duration(seconds: 2);

  /// An answer younger than this is not asked again: a choice, the mean
  /// heartbeat interval (spec "Heartbeat"), within which what a neighbour
  /// hears seldom changes.
  static const fresh = Duration(seconds: 10);

  final Map<String, _Ask> _of = {};

  AskState state(String id) => _of[id]?.state ?? AskState.never;

  /// Of `ids`, those to ask now, whose series starts: each not under way
  /// and without an answer within [fresh]. The person opened the map,
  /// refreshed it or opened one node's sheet.
  List<String> look(Iterable<String> ids, DateTime now) => [
    for (final id in ids)
      if (_start(id, now)) id,
  ];

  bool _start(String id, DateTime now) {
    final a = _of[id];
    if (a != null) {
      if (a.state == AskState.asking) return false;
      if (a.state == AskState.answered && now.difference(a.at!) < fresh) {
        return false;
      }
    }
    _of[id] = _Ask(AskState.asking, left: rounds - 1);
    return true;
  }

  /// `id`'s ask is written: its wait starts.
  void sent(String id, DateTime now) {
    final a = _of[id];
    if (a != null && a.state == AskState.asking) a.at = now.add(wait);
  }

  void answered(String id, DateTime now) =>
      _of[id] = _Ask(AskState.answered, at: now);

  /// When [due] next has something to do.
  DateTime? get next {
    DateTime? n;
    for (final a in _of.values) {
      final at = a.at;
      if (a.state == AskState.asking && at != null) {
        if (n == null || at.isBefore(n)) n = at;
      }
    }
    return n;
  }

  /// Those to ask again now, their wait over, and those given up now, as
  /// their asks ran out.
  ({List<String> again, List<String> gaveUp}) due(DateTime now) {
    final again = <String>[];
    final gaveUp = <String>[];
    for (final MapEntry(key: id, value: a) in _of.entries) {
      final at = a.at;
      if (a.state != AskState.asking || at == null || at.isAfter(now)) {
        continue;
      }
      if (a.left > 0) {
        a.left--;
        a.at = null;
        again.add(id);
      } else {
        a.state = AskState.gaveUp;
        a.at = now;
        gaveUp.add(id);
      }
    }
    return (again: again, gaveUp: gaveUp);
  }

  /// The map is shut: no series goes on, and the next look starts anew.
  void stop() => _of.removeWhere((_, a) => a.state == AskState.asking);

  /// The link is gone, and the answers with it.
  void clear() => _of.clear();
}
