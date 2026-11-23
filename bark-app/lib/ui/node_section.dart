// Settings' Node section: the connection line and its help, how the
// node fares, the nodes the phone is paired with, Disconnect or Stop
// looking, and the person's scan with the nodes it found.

import 'package:flutter/material.dart';

import '../app.dart';
import '../l10n.dart';
import '../links.dart' show Linking;
import '../ble_link.dart';
import '../link.dart';
import 'common.dart';
import 'settings_parts.dart';

class NodeSection extends StatelessWidget {
  final AppModel app;
  final ConnectAsk connect;
  const NodeSection(this.app, this.connect, {super.key});

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: _connection(context),
  );

  /// What a connection being made is doing, for the line under the
  /// node.
  static String _linking(Linking l) => switch (l) {
    Linking.none => '',
    Linking.connecting => tr.nodeConnecting,
    Linking.pairing => tr.nodeTypeCode,
    Linking.paired || Linking.attaching => tr.nodePairedConnecting,
  };

  List<Widget> _connection(BuildContext context) {
    final s = app.session;
    return [
      SettingsHeading(tr.nodeHeading),
      if (app.links.connecting)
        Text(_linking(app.links.linking))
      else
        Row(
          children: [
            Expanded(child: Text(connectionLine(app))),
            ConnectionHelpButton(app),
          ],
        ),
      if (s.connected)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Ticking(when: s.bootLeftS > 0, (_) => Text(health(s))),
        ),
      ..._paired(context),
      // Below the paired nodes, so that Disconnect, which only ends the
      // link, is not taken for Unpair.
      if (s.connected)
        Align(
          alignment: Alignment.centerLeft,
          child: OutlinedButton(
            onPressed: app.links.disconnect,
            child: Text(tr.nodeDisconnect),
          ),
        )
      else if (lookingLine(app) != null)
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton(
            onPressed: app.links.stopLooking,
            child: Text(tr.nodeStopLooking),
          ),
        )
      else
        const SizedBox(height: 8),
      Row(
        children: [
          FilledButton(
            onPressed: app.links.scanning || app.links.connecting
                ? app.links.stopScan
                : () => trySay(context, app.links.scan, tr.nodeScanFailed),
            child: Text(app.links.scanning ? tr.nodeStopScan : tr.nodeScan),
          ),
          const SizedBox(width: 16),
          if (app.links.scanning || app.links.connecting)
            const SizedBox.square(
              dimension: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
        ],
      ),
      // The connected node's row does nothing; another's connects to
      // it, closing the link to this one first: a public node not the
      // phone's as a guest, or paired with at once by its Pair.
      for (final FoundNode n in app.links.found.values) _found(context, n),
      if (app.links.noneFound) ...[
        const SizedBox(height: 8),
        Text(tr.nodeNoneFound),
        Text(tr.nodeNoneFoundHow, style: Theme.of(context).textTheme.bodySmall),
      ],
    ];
  }

  /// The nodes this phone is paired with, the home first, each with how
  /// it stands; a tap on another connects to it as a paired phone (spec "Phone
  /// link", Pairing). The phone's own row of the node's phones has Unpair
  /// ([_phones]).
  List<Widget> _paired(BuildContext context) {
    final s = app.session;
    final home = app.links.homeName;
    final paired = app.session.nodes.paired;
    final names = [
      if (paired.contains(home)) home!,
      for (final n in paired)
        if (n != home) n,
    ];
    if (names.isEmpty) return [];
    final on = s.connected && !s.guest ? nodeName(s.node!.id) : null;
    return [
      Padding(
        padding: const EdgeInsets.only(top: 8),
        child: Text(
          tr.nodePairedWith,
          style: Theme.of(context).textTheme.titleSmall,
        ),
      ),
      for (final n in names)
        if (app.session.nodes[n]!.lost)
          _lostRow(context, n, home: n == home)
        else
          ListTile(
            contentPadding: EdgeInsets.zero,
            leading: Icon(n == home ? Icons.home_outlined : Icons.router),
            title: Text(shownName(n)),
            subtitle: switch (_pairedState(n, home: n == home, on: n == on)) {
              '' => null,
              final w => Text(w),
            },
            onTap: n == on || app.links.connecting
                ? null
                : () => connect(context, () => app.links.connectPaired(n)),
          ),
    ];
  }

  /// The row of `n`, a paired node that does not know this phone: Pair
  /// again while it is in reach, as the person most likely wants it back,
  /// and Forget, to give it up; a tap explains.
  Widget _lostRow(BuildContext context, String n, {required bool home}) =>
      ListTile(
        contentPadding: EdgeInsets.zero,
        leading: Icon(home ? Icons.home_outlined : Icons.router),
        title: Text(shownName(n)),
        subtitle: Text(_pairedState(n, home: home, on: false)),
        onTap: () => _lostHelp(context, n),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (app.links.lostInReach(n))
              TextButton(
                onPressed: app.links.connecting
                    ? null
                    : () => connect(
                        context,
                        () => app.links.pairLost(n),
                        pairs: true,
                      ),
                child: Text(tr.pairAgain),
              ),
            TextButton(
              onPressed: () => _askForget(context, n, home: home),
              child: Text(tr.nodeForget),
            ),
          ],
        ),
      );

  /// Why `n`, a paired node, does not know this phone, and what each
  /// button does.
  Future<void> _lostHelp(BuildContext context, String n) =>
      showHelp(context, tr.nodeLostTitle(shownName(n)), tr.nodeLostHelp);

  /// How a paired node stands, in words. A paired node the search hears
  /// it connects to, so one not connected while it looks is out of reach,
  /// unless it passes the node over for its version; while the phone is on
  /// another node the search stops and cannot tell.
  String _pairedState(String n, {required bool home, required bool on}) {
    final r = app.session.nodes[n];
    return on
        ? (home ? tr.nodeHomeConnected : tr.nodeConnected)
        : r?.lost ?? false
        ? (home ? tr.nodeHomeLost : tr.nodeLost)
        : r?.unserved ?? false
        ? (home ? tr.nodeHomeUnserved : tr.nodeUnserved)
        : app.links.searchOn
        ? (home ? tr.nodeHomeAway : tr.nodeAway)
        : app.session.stopped
        ? (home ? tr.nodeHomeNotLooking : tr.nodeNotLooking)
        : (home ? tr.nodeYourHome : '');
  }

  Widget _found(BuildContext context, FoundNode n) {
    final guest = n.public && !app.links.pairedNodes.contains(n.name);
    final busy = app.links.connecting || app.links.connectedTo(n.deviceId);
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(n.public ? Icons.storefront : Icons.router),
      title: Text(shownName(n.name)),
      subtitle: Text.rich(
        TextSpan(
          text: [
            if (app.links.connectedTo(n.deviceId)) tr.nodeConnected,
            if (n.public) tr.nodeFoundPublic,
            if (app.session.nodes[n.name]?.unserved ?? false)
              tr.nodeFoundUnserved,
            signalWord(n.rssi),
          ].join(', '),
          children: [
            if (n.rssi != null)
              TextSpan(
                text: '  ${n.rssi} dBm',
                style: Theme.of(context).textTheme.bodySmall,
              ),
          ],
        ),
      ),
      onTap: busy
          ? null
          : () => connect(
              context,
              () => guest ? app.links.connectGuest(n) : app.links.connect(n),
            ),
      trailing: guest
          ? TextButton(
              onPressed: app.links.connecting
                  ? null
                  : () => _askPair(context, n),
              child: Text(tr.nodePair),
            )
          : null,
    );
  }

  /// Pairs with `n`, a public node, by the code on its screen, once the
  /// person has read what pairing gives; the home is set apart.
  Future<void> _askPair(BuildContext context, FoundNode n) async {
    if (app.session.nodes[n.name]?.asGuest?.room == 0) {
      say(context, tr.fullNode);
      return;
    }
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(tr.nodePairTitle(shownName(n.name))),
        content: Text(tr.nodePairHow),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(tr.nodePair),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    await connect(context, () => app.links.connect(n), pairs: true);
  }

  /// Forgets `name`, a node no longer this phone's, after a dialog that
  /// says what follows; where the phone's own pairing with it may stay,
  /// says how to forget that too.
  Future<void> _askForget(
    BuildContext context,
    String name, {
    required bool home,
  }) async {
    // Each part is a whole sentence.
    String said(List<String> s) =>
        [s.first, if (home) tr.staysYourHome, ...s.skip(1)].join(' ');
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(tr.nodeForgetTitle(shownName(name))),
        content: Text(said([tr.nodeForgetHow])),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(tr.nodeForget),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    await trySay(context, () async {
      final gone = await app.links.forget(name);
      if (!context.mounted) return;
      switch (gone) {
        case true:
          say(context, said([tr.nodeForgotten]));
        case false:
          await askToForget(
            context,
            app,
            said([tr.nodeForgotten, tr.forgetInBluetoothToo]),
          );
        case null:
          await askToForget(
            context,
            app,
            said([tr.nodeForgotten, tr.nodeForgetLater]),
          );
      }
    }, tr.nodeForgetFailed);
  }
}
