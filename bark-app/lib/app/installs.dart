// A node's install of an image it gathered over the mesh, or of one the
// phone sent it, as the app follows it, for the line and for the search's
// wait (spec "Updates over the mesh", the install; roadmap D182, D224).

part of '../app.dart';

/// How long the line says a node is updated, once it is back on the new
/// version: a minute, long enough to be seen when the person looks.
const Duration updatedShown = Duration(minutes: 1);

/// A node's install of an image it gathered over the mesh, or of one
/// the phone `sent` it (`updater.dart`), as the app follows it (spec
/// "Updates over the mesh", the install; roadmap D182, D224): the node's
/// name, the version, when the link ended for the reboot, whether the
/// search gave up waiting for it, and when the node came back on that
/// version.
typedef Install = ({
  String node,
  int version,
  bool sent,
  DateTime? gone,
  bool givenUp,
  DateTime? back,
});

/// The firmware version whose low 16 bits are `low`, as Status tells it,
/// of a node that runs `image`: the first above it.
int installVersion(int image, int low) {
  final v = image - (image & 0xffff) + low;
  return v > image ? v : v + 0x10000;
}

mixin _Installs on _AppCore {
  /// The install the app follows, the last a node told of.
  @override
  Install? install;

  void _installs() {
    final n = session.connected ? session.node : null;
    final i = install;
    if (n == null) {
      if (i != null && i.gone == null && i.back == null) {
        install = (
          node: i.node,
          version: i.version,
          sent: false,
          gone: now(),
          givenUp: false,
          back: null,
        );
        session.note('${i.node} reboots to install version ${i.version}');
      }
      return;
    }
    final name = nodeName(n.id);
    // An owner's link to another node, as the person tapped it, ends the
    // wait for one that installs (roadmap D202).
    if (i != null && i.node != name && !session.guest) {
      install = null;
      return;
    }
    final status = session.status;
    if (status == null) return;
    final low = status.installing;
    if (low != 0) {
      final version = installVersion(n.image, low);
      if (i?.node != name || i?.version != version || i?.gone != null) {
        install = (
          node: name,
          version: version,
          sent: false,
          gone: null,
          givenUp: false,
          back: null,
        );
        session.note('$name drains to install version $version');
      }
    } else if (i?.node == name && i!.back == null) {
      final on = n.image == i.version && i.gone != null;
      install = on
          ? (
              node: name,
              version: i.version,
              sent: i.sent,
              gone: i.gone,
              givenUp: i.givenUp,
              back: now(),
            )
          : null;
      // The updater tells of an image the phone sent.
      if (!i.sent) {
        session.note(
          on
              ? '$name runs version ${i.version}, installed over the mesh'
              : '$name does not install version ${i.version} now',
        );
      }
    }
  }

  /// The node `id` ended its link to reboot into `version`, which the
  /// phone sent it: the search waits for it as for one that installs.
  void _rebooted(List<int> id, int version) {
    install = (
      node: nodeName(id),
      version: version,
      sent: true,
      gone: now(),
      givenUp: false,
      back: null,
    );
    notifyListeners();
  }

  /// The node in use tells it drains to install the version [install]
  /// names.
  @override
  bool get _installsNow {
    final i = install;
    final n = session.connected ? session.node : null;
    return i != null && i.gone == null && n != null && nodeName(n.id) == i.node;
  }

  /// A paired node rebooted to install a version and is not yet back,
  /// while the search has not given up waiting for it: away for the swap,
  /// or for as long as its firmware may take after an image the phone
  /// sent, which the person waits for ([backWithin]).
  @override
  Awaited? get _awaited {
    final i = install;
    final gone = i?.gone;
    if (i == null || gone == null || i.givenUp || i.back != null) return null;
    if (!nodes.own.contains(i.node)) return null;
    return (node: i.node, gone: gone, away: i.sent ? backWithin : swapAway);
  }

  /// The search gives up waiting for the node that installs: it is out of
  /// reach as any node, or back and passed over ([waitsForInstall]).
  @override
  void _giveUp(String node) {
    final i = install!;
    install = (
      node: i.node,
      version: i.version,
      sent: i.sent,
      gone: i.gone,
      givenUp: true,
      back: null,
    );
    session.note(
      nodes[node]?.unserved ?? false
          ? '$node back from its install, but it and this app do not serve '
                'each other: passed over'
          : '$node not back from its install: out of reach',
    );
  }

  /// The line while a paired node is away to install a version; null
  /// otherwise.
  String? get installingLine {
    if (session.connected || !waitsForInstall(situation())) return null;
    final i = install!;
    // Only an image the phone sent is waited for past its swap.
    final late = i.sent && now().difference(i.gone!) >= swapAway;
    return '${shownName(i.node)} is installing version ${i.version}, '
        '${late ? 'not back yet' : 'back in about a minute'}';
  }

  /// Why the node is or was away for an install, for the help; null when
  /// none is under way.
  String? get installHelp {
    if (!_installsNow && installingLine == null) return null;
    final i = install!;
    if (i.sent) {
      return '${shownName(i.node)} restarts into version ${i.version}, which '
          'this phone sent it, for about a minute. The phone waits for it '
          'and reconnects by itself.';
    }
    return '${shownName(i.node)} got version ${i.version} from the nodes '
        'around it. It passes on the texts it holds, at most ten minutes, '
        'then restarts for about a minute. The phone reconnects by itself; '
        'nothing is lost.';
  }

  /// What the line adds of the connected node's install: that it is
  /// installing a version, or that it came back on it within
  /// [updatedShown].
  String get _installNote {
    final i = install;
    if (i == null || !session.connected) return '';
    if (_installsNow) return ', installing version ${i.version}';
    final back = i.back;
    final n = session.node;
    if (back != null &&
        n != null &&
        nodeName(n.id) == i.node &&
        now().difference(back) < updatedShown) {
      return ', updated to version ${i.version}';
    }
    return '';
  }
}
