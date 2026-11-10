// A firmware update of the connected node over the phone link (spec
// "Phone link", Updates): the image from the latest release or a file,
// sent in pieces, resumed from what the node holds after a lost link,
// committed, and the reboot into it, until the node is back; then
// whether the node hears anyone on its new image.

import 'dart:async';
import 'dart:math';

import 'package:crypto/crypto.dart' show sha256;
import 'package:flutter/foundation.dart';

import 'link.dart';
import 'node_link.dart';
import 'releases.dart';
import 'session.dart';

/// Pieces written between two reads of Update: the node's host queues 8
/// ATT PDUs for a link and drops one more, so seven pieces and the read
/// fit (spec "Phone link", Updates).
const int window = 7;

/// Windows in a row that move the bytes held not at all before the app
/// gives up: a node that takes no piece of 7 × 20.
const int stalledWindows = 20;

/// How long the app waits for the link's end after a reboot whose write
/// failed: the longest supervision timeout, 32 s (Bluetooth Core
/// Specification 5.0, Vol 6 Part B §4.5.2), by which a node that reset
/// without ending the link has lost it anyway.
const Duration rebootEnd = Duration(seconds: 32);

/// How long a node the app updated may hear no neighbour, sleepy ones
/// counted, before the app says it hears no one: as long as an image
/// gathered over the mesh listens before it gives itself up, three
/// worst-case rotations of heartbeats, 13 × 15 s each, and a heartbeat's
/// lateness, 872.8 ms (spec "Updates over the mesh", the install).
const Duration hearsNoOneAfter = Duration(
  microseconds: 3 * 13 * 15000000 + 872800,
);

/// An image in `file`, its hash checked.
FirmwareImage parseImage(List<int> file) =>
    FirmwareImage.parse(file, (b) => sha256.convert(b).bytes);

/// What the app does next with `image`, by what the node holds.
enum Step { begin, send, commit, reboot }

/// The next step for `image` on a node whose update reads `p`: from the
/// bytes it holds if they are the image's, else from the start.
Step next(UpdateProgress p, FirmwareImage image) {
  if (!p.holds(image)) return Step.begin;
  return switch (p.state) {
    UpdateState.idle => Step.begin,
    UpdateState.receiving => Step.send,
    UpdateState.staged => Step.commit,
    UpdateState.committed => Step.reboot,
  };
}

/// The node took none of the pieces of [stalledWindows] windows.
class Stalled implements Exception {
  const Stalled();
  @override
  String toString() => 'The node takes no pieces';
}

/// Where the update stands, as the person sees it.
enum Phase {
  /// No image chosen, or one chosen and not yet sent.
  idle,

  /// Sending the image.
  sending,

  /// The link was lost while sending: it goes on when the node is back.
  paused,

  /// The node holds every byte; it checks them at the commit.
  staged,

  /// The node checks the image.
  committing,

  /// The node took it: the reboot swaps it in.
  committed,

  /// The node reboots and swaps; the app waits for it.
  rebooting,

  /// The node is back on the new image.
  done,

  /// The node refused, or the link failed in a way that does not resume.
  failed,
}

/// A finished update: the node `id` went from version `from` to `to` at
/// `at`.
typedef Updated = ({List<int> id, int from, int to, DateTime at});

/// The update of the connected node.
class Updater extends ChangeNotifier {
  final Session session;

  /// The MTU agreed for the link.
  final int Function() mtu;

  /// The latest release the app knows of (`releases.dart`).
  final Release? Function() release;

  Updater(this.session, this.mtu, {Release? Function()? release})
    : release = release ?? (() => null) {
    session.addListener(_onSession);
  }

  FirmwareImage? image;

  /// Where the image came from, for the person.
  String? from;

  Phase phase = Phase.idle;

  /// The image's bytes the node holds, as last read.
  int held = 0;

  /// The last thing the node said, or what went wrong.
  String? message;

  /// The version the node ran before the reboot.
  int? _before;

  Updated? _updated;

  /// The node updated last has heard a neighbour since it came back.
  bool _heard = false;

  /// Shows [hearsNoOne] when its time comes.
  Timer? _listening;

  /// The last update finished, while the connected node is the one it
  /// updated and still runs its version.
  Updated? get updated {
    final u = _updated;
    final n = session.node;
    if (u == null || n == null) return null;
    return listEquals(n.id, u.id) && n.image == u.to ? u : null;
  }

  /// The node updated last has heard no neighbour, sleepy or not, for
  /// [hearsNoOneAfter] since it came back on its new image, and is
  /// connected: the image may have broken its radio. A node updated from
  /// the phone keeps its image whatever it hears, as it may stand alone,
  /// so the person is told (spec "Phone link", Updates).
  bool get hearsNoOne {
    final u = updated;
    if (u == null || _heard || (session.status?.heard ?? 0) > 0) {
      return false;
    }
    return session.now().difference(u.at) >= hearsNoOneAfter;
  }

  /// The person has read [hearsNoOne]'s warning: it goes until the next
  /// update.
  void dismissHearsNoOne() {
    _heard = true;
    notifyListeners();
  }

  /// What [hearsNoOne] tells the person, and the way back: the node
  /// stages no image below its own version, so the one it ran before goes
  /// on by USB.
  String hearsNoOneWarning(Updated u) =>
      'Since its update to version ${u.to} it has heard no other node: the '
      'new firmware may have broken its radio. To bring it back, copy the '
      'version it ran before, bark-fw-${u.from}.uf2 from that release, to '
      'it over USB after a double tap of its reset button; it takes no '
      'older version from the phone. A newer release, once there is one, '
      'can be sent from here.';

  /// The app is in an exchange with the node that runs without the
  /// person: sending the image, waiting for its check, or for its reboot.
  /// Closing the link then cuts it.
  bool get underWay =>
      phase == Phase.sending ||
      phase == Phase.committing ||
      phase == Phase.rebooting;

  /// Whether the chosen image, its progress and its actions are shown:
  /// not once the node runs it, when they say nothing more.
  bool get showsImage => image != null && phase != Phase.done;

  /// The chosen image is newer than the node runs, so the node would take
  /// it.
  bool get newer {
    final n = session.node;
    return n == null || (image?.version ?? 0) > n.image;
  }

  /// The latest release, if it carries an image newer than the node
  /// runs: what "Latest release" chooses.
  Release? get latest {
    final r = release();
    final n = session.node;
    if (r == null || r.image == null || n == null) return null;
    return r.firmware > n.image ? r : null;
  }

  /// Why there is no [latest], for the person; null when there is one,
  /// or no node to compare with.
  String? get noLatest {
    final r = release();
    final n = session.node;
    if (n == null || latest != null) return null;
    if (r == null) return 'No release known yet.';
    if (r.image == null) return 'Release ${r.n} carries no node firmware.';
    if (r.firmware == n.image) {
      return 'The node already runs the latest release\'s firmware, '
          'version ${r.firmware}.';
    }
    return 'The node runs a newer version than the latest release\'s '
        'firmware, ${r.firmware}.';
  }

  /// The link has closed since the reboot was asked for.
  bool _down = false;

  bool _running = false;

  void _set(Phase p, [String? m]) {
    phase = p;
    message = m;
    notifyListeners();
  }

  /// Chooses `i`, from `source`, for the next send.
  void choose(FirmwareImage i, String source) {
    image = i;
    from = source;
    held = 0;
    _set(Phase.idle);
  }

  NodeLink _link() {
    final l = session.link;
    final n = session.node;
    if (l == null || n == null) throw StateError('not connected');
    if (session.guest) {
      throw StateError('only the phone paired with the node updates it');
    }
    if (n.version < updateVersion) {
      throw StateError(
        'the node\'s firmware is from before updates over Bluetooth: '
        'update it by USB once',
      );
    }
    return l;
  }

  /// Sends the image from what the node holds, then commits it.
  Future<void> send() async {
    if (_running || image == null) return;
    _running = true;
    try {
      await _send(image!);
    } on AttException catch (e) {
      await _refused(e, Phase.staged);
    } on Stalled catch (e) {
      _set(Phase.failed, '$e');
    } catch (e) {
      session.note('Update: paused at $held B, $e');
      _set(
        Phase.paused,
        'The link was lost at $held of ${image!.length} bytes; it goes on '
        'when the node is back',
      );
    } finally {
      _running = false;
    }
  }

  Future<void> _send(FirmwareImage img) async {
    final link = _link();
    _set(Phase.sending);
    var p = await link.readUpdate();
    if (next(p, img) == Step.begin) {
      await link.writeUpdate(encodeBegin(img.header));
      session.note('Update: began version ${img.version}, ${img.length} B');
      p = await link.readUpdate();
    } else {
      session.note('Update: goes on from ${p.held} of ${img.length} B');
    }
    await link.hurry();
    final n = pieceBytes(min(mtu(), attMtu));
    var stalled = 0;
    while (next(p, img) == Step.send) {
      final from = p.held;
      held = from;
      notifyListeners();
      for (var k = 0, at = from; k < window && at < img.length; k++) {
        final end = min(at + n, img.length);
        await link.writePiece(encodePiece(at, img.bytes.sublist(at, end)));
        at = end;
      }
      p = await link.readUpdate();
      stalled = p.held > from ? 0 : stalled + 1;
      if (stalled >= stalledWindows) throw const Stalled();
    }
    held = p.held;
    switch (next(p, img)) {
      case Step.commit:
        await _commit(link, img);
      case Step.reboot:
        _set(Phase.committed, 'The node holds version ${img.version}');
      case Step.begin || Step.send:
        // The node forgot the image: a flash that failed.
        _set(Phase.failed, p.refused?.why ?? 'The node lost the firmware');
    }
  }

  Future<void> _commit(NodeLink link, FirmwareImage img) async {
    _set(Phase.committing);
    await link.writeUpdate(encodeCommit());
    session.note('Update: version ${img.version} committed');
    _set(
      Phase.committed,
      'The node checked version ${img.version} and keeps it for its next '
      'boot',
    );
  }

  /// Commits the image the node holds, again after a refusal for its
  /// battery.
  Future<void> commit() async {
    final img = image;
    if (_running || img == null) return;
    _running = true;
    try {
      await _commit(_link(), img);
    } on AttException catch (e) {
      await _refused(e, Phase.staged);
    } catch (e) {
      _set(Phase.failed, 'Committing failed: $e');
    } finally {
      _running = false;
    }
  }

  /// The node refused: why, from Update. On a low battery the node keeps
  /// what it had, and the update stays at `kept`, to go on once it is
  /// on USB or charged.
  Future<void> _refused(AttException e, Phase kept) async {
    if (e.code != AttError.refused) {
      _set(Phase.failed, 'The node answered $e');
      return;
    }
    Refusal? why;
    try {
      why = (await session.link?.readUpdate())?.refused;
    } catch (_) {}
    session.note('Update: refused, ${why?.name ?? '?'}');
    _set(
      why == Refusal.battery ? kept : Phase.failed,
      'The node refused: ${why?.why ?? 'it did not say why'}',
    );
  }

  /// Asks the node to reboot into the image; the app connects again once
  /// it is back.
  Future<void> reboot() async {
    if (_running) return;
    _running = true;
    try {
      final link = _link();
      _before = session.node?.image;
      _down = false;
      try {
        await link.writeUpdate(encodeReboot());
      } catch (e) {
        if (e is AttException && e.code == AttError.refused) rethrow;
        if (!await _ends(link)) rethrow;
        session.note('Update: the reboot\'s answer was lost to its end, $e');
      }
      session.note('Update: reboot asked');
      _set(
        Phase.rebooting,
        'The node swaps the new firmware in, about a minute; the app '
        'connects again when it is back',
      );
    } on AttException catch (e) {
      await _refused(e, Phase.committed);
    } catch (e) {
      _set(Phase.failed, 'Reboot failed: $e');
    } finally {
      _running = false;
    }
  }

  /// Whether `link` ends within [rebootEnd]. The node answers a reboot
  /// and then ends the link, and Android may report the write failed as
  /// that end overtakes the answer, with its catch-all GATT error 133
  /// (bench, 2026-11-07): a write that failed so is the reboot taken.
  Future<bool> _ends(NodeLink link) =>
      link.closed.then((_) => true).timeout(rebootEnd, onTimeout: () => false);

  void _onSession() {
    final n = session.node;
    if (n == null) {
      _down = true;
      return;
    }
    if (updated != null && (session.status?.heard ?? 0) > 0) _heard = true;
    if (phase == Phase.paused && !_running) {
      unawaited(send());
    } else if (phase == Phase.rebooting && _down) {
      final want = image?.version;
      if (n.image == want) {
        _updated = (
          id: n.id,
          from: _before ?? 0,
          to: n.image,
          at: session.now(),
        );
        _heard = false;
        _listening?.cancel();
        _listening = Timer(hearsNoOneAfter, notifyListeners);
        _set(Phase.done);
        // After the phase moved: the note notifies this listener again.
        session.note('Update: version ${n.image} runs');
        return;
      }
      _set(
        Phase.failed,
        'The node came back on version ${n.image}, not $want: the '
        'new firmware did not start, and the node swapped '
        'version ${_before ?? n.image} back',
      );
    }
  }

  @override
  void dispose() {
    _listening?.cancel();
    session.removeListener(_onSession);
    super.dispose();
  }
}

/// The name a picked image is shown by: `name`, as the file picker gives
/// it, with the `.img` that images carry (`bark-fw/scripts/image.py`;
/// a release's `bark-fw-<N>.img`). Android's picker gives a copy of the
/// file, named with the extension of its type, `.bin` for one whose type
/// Android does not know, as an image's.
String pickedName(String name) {
  final dot = name.lastIndexOf('.');
  final base = dot > 0 ? name.substring(0, dot) : name;
  return base.isEmpty ? 'a picked file' : '$base.img';
}

/// The image of `r`, downloaded by `get` and checked, with its file name.
Future<(FirmwareImage, String)> downloadImage(
  Release r, [
  Future<List<int>> Function(String url) get = httpGet,
]) async {
  final url = r.image;
  if (url == null) throw StateError('release ${r.n} has no ${r.imageName}');
  return (parseImage(Uint8List.fromList(await get(url))), r.imageName);
}
