// Settings' Node section: the connection line and its help, how the
// node fares, the nodes the phone is paired with, Disconnect or Stop
// looking, and the person's scan with the nodes it found.

import 'package:flutter/material.dart';

import '../app.dart';
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
    Linking.connecting => 'Connecting…',
    Linking.pairing => 'Type the code on the node\'s screen',
    Linking.paired || Linking.attaching => 'Paired, connecting…',
  };

  List<Widget> _connection(BuildContext context) {
    final s = app.session;
    return [
      SettingsHeading('Node'),
      if (app.connecting)
        Text(_linking(app.linking))
      else
        Row(
          children: [
            Expanded(child: Text(connectionLine(app))),
            ConnectionHelpButton(app),
          ],
        ),
      if (s.connected)
        Padding(padding: const EdgeInsets.only(top: 8), child: Text(health(s))),
      ..._paired(context),
      // Below the paired nodes, so that Disconnect, which only ends the
      // link, is not taken for Unpair.
      if (s.connected)
        Align(
          alignment: Alignment.centerLeft,
          child: OutlinedButton(
            onPressed: app.disconnect,
            child: const Text('Disconnect'),
          ),
        )
      else if (lookingLine(app) != null)
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton(
            onPressed: app.stopLooking,
            child: const Text('Stop looking'),
          ),
        )
      else
        const SizedBox(height: 8),
      Row(
        children: [
          FilledButton(
            onPressed: app.scanning || app.connecting
                ? app.stopScan
                : () => trySay(context, app.scan, 'Scan failed'),
            child: Text(app.scanning ? 'Stop' : 'Scan'),
          ),
          const SizedBox(width: 16),
          if (app.scanning || app.connecting)
            const SizedBox.square(
              dimension: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
        ],
      ),
      // The connected node's row does nothing; another's connects to
      // it, closing the link to this one first: a public node not the
      // phone's as a guest, or paired with at once by its Pair.
      for (final FoundNode n in app.found.values) _found(context, n),
      if (app.noneFound) ...[
        const SizedBox(height: 8),
        const Text('No node found nearby.'),
        Text(
          'Switch one on, or come closer.',
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ],
    ];
  }

  /// The nodes this phone is paired with, the home first, each with how
  /// it stands; a tap on another connects to it as its owner (spec "Phone
  /// link", Pairing). The phone's own row of the node's phones has Unpair
  /// ([_phones]).
  List<Widget> _paired(BuildContext context) {
    final s = app.session;
    final home = app.homeName;
    final paired = app.nodes.paired;
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
          'Paired with',
          style: Theme.of(context).textTheme.titleSmall,
        ),
      ),
      for (final n in names)
        if (app.nodes[n]!.lost)
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
            onTap: n == on || app.connecting
                ? null
                : () => connect(context, () => app.connectPaired(n)),
          ),
    ];
  }

  /// The row of `n`, a paired node that no longer knows this phone: Pair
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
            if (app.lostInReach(n))
              TextButton(
                onPressed: app.connecting
                    ? null
                    : () =>
                          connect(context, () => app.pairLost(n), pairs: true),
                child: const Text('Pair again'),
              ),
            TextButton(
              onPressed: () => _askForget(context, n, home: home),
              child: const Text('Forget'),
            ),
          ],
        ),
      );

  /// What happened to `n`, a node that no longer knows this phone, and
  /// what each button does.
  Future<void> _lostHelp(BuildContext context, String n) => showHelp(
    context,
    '${shownName(n)} no longer knows this phone',
    'Someone paired with it removed this phone, or the node lost its '
        'memory.\n\n'
        'Pair again: pair anew with the code on its screen. Shown while the '
        'node is in reach.\n\n'
        'Forget: stop listing the node here.',
  );

  /// How a paired node stands, in words. A paired node the search hears
  /// it connects to, so one not connected while it looks is out of reach,
  /// unless it passes the node over for its version; while the phone is on
  /// another node the search stops and cannot tell.
  String _pairedState(String n, {required bool home, required bool on}) {
    final r = app.nodes[n];
    return capital(
      [
        if (home) 'your home',
        if (on)
          'connected'
        else if (r?.lost ?? false)
          'no longer yours'
        else if (r?.unserved ?? false)
          'cannot be used yet. Tap to try again'
        else if (app.searchOn)
          'not in reach'
        else if (app.session.stopped)
          'not looking. Tap to connect',
      ].join(', '),
    );
  }

  Widget _found(BuildContext context, FoundNode n) {
    final guest = n.public && !app.pairedNodes.contains(n.name);
    final busy = app.connecting || app.connectedTo(n.deviceId);
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(n.public ? Icons.storefront : Icons.router),
      title: Text(shownName(n.name)),
      subtitle: Text.rich(
        TextSpan(
          text: [
            if (app.connectedTo(n.deviceId)) 'Connected',
            if (n.public) 'public',
            if (app.nodes[n.name]?.unserved ?? false) 'cannot be used yet',
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
              () => guest ? app.connectGuest(n) : app.connect(n),
            ),
      trailing: guest
          ? TextButton(
              onPressed: app.connecting ? null : () => _askPair(context, n),
              child: const Text('Pair'),
            )
          : null,
    );
  }

  /// Pairs with `n`, a public node, by the code on its screen, once the
  /// person has read what pairing gives; the home is set apart.
  Future<void> _askPair(BuildContext context, FoundNode n) async {
    if (app.nodes[n.name]?.asGuest?.room == 0) {
      say(context, fullNode);
      return;
    }
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Pair with ${shownName(n.name)}?'),
        content: const Text(
          'You can then change its settings and update it. Type the code on '
          'its screen.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Pair'),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    await connect(context, () => app.connect(n), pairs: true);
  }

  /// Forgets `name`, a node no longer this phone's, after a dialog that
  /// says what follows; where the phone's own pairing with it may stay,
  /// says how to forget that too.
  Future<void> _askForget(
    BuildContext context,
    String name, {
    required bool home,
  }) async {
    final stays = home ? ' It stays your home.' : '';
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Forget ${shownName(name)}?'),
        content: Text('This phone stops listing it.$stays'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Forget'),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    await trySay(context, () async {
      final gone = await app.forget(name);
      if (!context.mounted) return;
      switch (gone) {
        case true:
          say(context, 'Forgotten.$stays');
        case false:
          await askToForget(
            context,
            app,
            'Forgotten.$stays Forget the node in Bluetooth settings too: the '
            'phone kept its pairing.',
          );
        case null:
          await askToForget(
            context,
            app,
            'Forgotten.$stays The phone drops its pairing when it next hears '
            'the node, or forget it in Bluetooth settings now.',
          );
      }
    }, 'Forgetting failed');
  }
}
