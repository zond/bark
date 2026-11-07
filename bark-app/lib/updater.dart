// A firmware update of the connected node over the phone link (spec
// "Phone link", Updates): the image from the latest release or a file,
// sent in pieces, resumed from what the node holds after a lost link,
// committed, and the reboot into it, until the node is back.

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:crypto/crypto.dart' show sha256;
import 'package:flutter/foundation.dart';

import 'link.dart';
import 'node_link.dart';
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

/// The update of the connected node.
class Updater extends ChangeNotifier {
  final Session session;

  /// The MTU agreed for the link.
  final int Function() mtu;

  Updater(this.session, this.mtu) {
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
    if (n.version < updateVersion) {
      throw StateError(
        'the node speaks link version ${n.version}, before updates over '
        'Bluetooth: update it by USB once',
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
        _set(Phase.failed, p.refused?.why ?? 'The node lost the image');
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
        'The node swaps the new image in, about a minute; the app '
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
    if (phase == Phase.paused && !_running) {
      unawaited(send());
    } else if (phase == Phase.rebooting && _down) {
      final want = image?.version;
      _set(
        n.image == want ? Phase.done : Phase.failed,
        n.image == want
            ? 'The node runs version ${n.image}'
            : 'The node came back on version ${n.image}, not $want: the '
                  'new image did not start, and the node swapped '
                  'version ${_before ?? n.image} back',
      );
    }
  }

  @override
  void dispose() {
    session.removeListener(_onSession);
    super.dispose();
  }
}

/// The repository whose releases carry the node's images.
const releasesUrl = 'https://api.github.com/repos/zond/bark/releases/latest';

/// The image asset of a release as GitHub's releases API gives it:
/// `bark-fw-<N>.img`, its name and where to download it.
({String name, String url})? imageAsset(Map<String, dynamic> release) {
  final pattern = RegExp(r'^bark-fw-[1-9][0-9]*\.img$');
  for (final a in (release['assets'] as List? ?? const [])) {
    final m = a as Map<String, dynamic>;
    final name = m['name'] as String? ?? '';
    final url = m['browser_download_url'] as String?;
    if (pattern.hasMatch(name) && url != null) return (name: name, url: url);
  }
  return null;
}

/// The latest release's image, downloaded and checked, with its asset's
/// name.
Future<(FirmwareImage, String)> latestRelease() async {
  final client = HttpClient();
  try {
    Future<List<int>> get(String url, {bool json = false}) async {
      final rq = await client.getUrl(Uri.parse(url));
      // GitHub's API refuses a request without a user agent.
      rq.headers.set(HttpHeaders.userAgentHeader, 'bark-app');
      if (json) rq.headers.set(HttpHeaders.acceptHeader, 'application/json');
      final rs = await rq.close();
      if (rs.statusCode != HttpStatus.ok) {
        throw HttpException('$url: HTTP ${rs.statusCode}');
      }
      return [for (final chunk in await rs.toList()) ...chunk];
    }

    final release = jsonDecode(
      utf8.decode(await get(releasesUrl, json: true)),
    ) as Map<String, dynamic>;
    final a = imageAsset(release);
    if (a == null) {
      throw StateError('the latest release has no bark-fw-<N>.img');
    }
    return (parseImage(Uint8List.fromList(await get(a.url))), a.name);
  } finally {
    client.close();
  }
}
