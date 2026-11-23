// Everything the link says, raw: Node, Status, Given back, the message
// IDs and payloads, and the log of the link's events. The headings and
// labels are in the language spoken; the values, the sent and received
// records and the log stay as a fault report carries them.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app.dart';
import '../l10n.dart';
import '../link.dart';
import '../session.dart';
import 'common.dart';
import 'update_screen.dart';

class DebugScreen extends StatelessWidget {
  final AppModel app;
  const DebugScreen(this.app, {super.key});

  // The log tells only its own listeners; the waits count down.
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: Listenable.merge([
      app.changes,
      app.session.log,
      Seconds.instance,
    ]),
    builder: (context, _) => _build(context),
  );

  Widget _build(BuildContext context) {
    final s = app.session;
    Widget heading(String t) => Padding(
      padding: const EdgeInsets.only(top: 24, bottom: 8),
      child: Text(t, style: Theme.of(context).textTheme.titleLarge),
    );
    return Scaffold(
      appBar: AppBar(title: Text(tr.debug)),
      body: SafeArea(
        top: false,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            NoticeCard(app),
            heading(tr.debugMe),
            ..._me(),
            heading(tr.node),
            ..._node(context),
            heading(tr.debugStatus),
            ..._status(),
            heading(tr.debugBench),
            ..._bench(context),
            heading(tr.debugGivenBackHeading),
            _row(
              tr.debugValue,
              !s.connected
                  ? tr.notConnected
                  : switch (s.givenBack) {
                      null => tr.debugEmpty,
                      final g => tr.debugGivenBack(
                        msgIdHex(g.msgId),
                        g.why.name,
                        g.waitS,
                      ),
                    },
            ),
            heading(tr.debugSent),
            if (s.outbox.isEmpty) Text(tr.debugNone),
            for (final o in s.outbox.reversed)
              _raw(
                '${msgIdHex(o.id)} to ${o.dst}',
                '${o.kind.name} ${o.sent} to ${_who(o.to)}, '
                    '${o.state.name}${_wait(s, o)}'
                    '${o.error == null ? '' : ', ${o.error}'}, '
                    'queued ${o.at.toLocal()}\n'
                    '${hex(o.payload)}',
              ),
            heading(tr.debugReceived),
            if (s.received.isEmpty) Text(tr.debugNone),
            for (final r in s.received.reversed)
              _raw(
                '${msgIdHex(r.id)} from ${r.from == null ? 'nobody: did not open' : _who(r.from)}',
                'sent ${r.sent}${r.intro ? ', a first message' : ''}'
                    '${r.mailbox ? ', from the mailbox' : ''}, '
                    '${r.at.toLocal()}\n${hex(r.payload)}',
              ),
            heading(tr.debugLog),
            if (s.log.isEmpty) Text(tr.debugNothingYet),
            for (final l in s.log.toList().reversed)
              Text(
                '${logTime(l.at)} ${l.text}',
                style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
              ),
          ],
        ),
      ),
    );
  }

  /// Tools for the bench: a firmware file picked on the phone, sent to
  /// the paired node as a release is (`bark-app/README.md`, "Updating a
  /// node").
  List<Widget> _bench(BuildContext context) {
    final s = app.session;
    final n = s.node;
    if (n == null || s.guest) {
      return [Text(tr.debugConnectPaired)];
    }
    return [
      Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute(builder: (_) => UpdateScreen(app, pick: true)),
          ),
          child: Text(tr.debugPickFirmware),
        ),
      ),
    ];
  }

  /// A contact's name and the start of its identity, in a record's
  /// English.
  String _who(String? key) {
    if (key == null) return 'the mailbox';
    final c = app.contacts.byKey(key);
    return '${c?.name ?? 'not a contact'} (${key.substring(0, 8)}…)';
  }

  List<Widget> _me() {
    final me = app.me;
    return [
      _row(tr.debugIdentity, '${me.key.substring(0, 16)}…'),
      _row(tr.debugCardAddress, '${me.home ?? tr.debugNoneNoHome}'),
      _row(tr.homeNode, '${me.home ?? tr.debugNoValue}'),
      _row(tr.debugUnsentAtHome, '${me.remaining}'),
      _row(tr.debugFetchRounds, '${me.rounds}'),
      _row(tr.debugLastSent, '${me.lastSent}'),
      _row(tr.debugAppVersion, switch (app.releases.app) {
        0 => tr.debugDevBuild,
        final n => '$n',
      }),
      _row(tr.debugLatestRelease, switch (app.releases.latest) {
        null => tr.debugNotKnown,
        final r => tr.debugRelease(
          r.n,
          r.app,
          r.firmware,
          '${app.releases.checked?.toLocal()}',
        ),
      }),
    ];
  }

  String _wait(Session s, Outgoing o) => switch ((o.state, o.why)) {
    (OutState.givenBack, Why.busy || null) => ' busy, ${s.waitLeftS(o)} s left',
    (OutState.givenBack, final Why w) => ' ${w.name}',
    _ => '',
  };

  Widget _raw(String title, String body) => ListTile(
    contentPadding: EdgeInsets.zero,
    title: Text(title, style: const TextStyle(fontFamily: 'monospace')),
    subtitle: SelectableText(
      body,
      style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
    ),
  );

  Widget _row(String k, String v) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 2),
    child: Row(
      children: [
        Expanded(child: Text(k)),
        Flexible(
          child: Text(
            v,
            textAlign: TextAlign.end,
            style: const TextStyle(fontWeight: FontWeight.bold),
          ),
        ),
      ],
    ),
  );

  String _yes(bool b) => b ? tr.debugYes : tr.debugNo;

  List<Widget> _node(BuildContext context) {
    final s = app.session;
    final n = s.node;
    if (n == null) return [Text(tr.nodeNotConnected)];
    final a = s.address;
    return [
      _row(tr.debugName, nodeName(n.id)),
      _row(tr.debugNodeId, hex(n.id)),
      // Bluetooth's own name for it.
      _row('MTU', '${app.links.mtu ?? '?'}'),
      _row(tr.debugLink, s.guest ? tr.debugLinkGuest : tr.debugLinkPaired),
      _row(tr.debugLinkVersion, '${n.version}'),
      _row(tr.debugFirmwareImage, '${n.image}'),
      _row(tr.debugClock, switch (s.nodeTime) {
        null => tr.notRead,
        final t => describeTime(t),
      }),
      _row(tr.debugPublic, switch (s.nodePublic) {
        null when s.guest => tr.debugPublicGuest,
        null => tr.notRead,
        final p => _yes(p),
      }),
      // What the node holds, in the log's words.
      _row(tr.debugHome, '${s.nodeHome ?? tr.notRead}'),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: Text(tr.debugAddress),
        subtitle: SelectableText(a?.toString() ?? tr.debugNoPlace),
        trailing: a == null
            ? null
            : IconButton(
                icon: const Icon(Icons.copy),
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: a.toString()));
                  say(context, tr.debugAddressCopied);
                },
              ),
      ),
    ];
  }

  List<Widget> _status() {
    final st = app.session.status;
    if (st == null) return [Text(tr.debugStatusNotRead)];
    return [
      _row(tr.debugReady, _yes(st.ready)),
      _row(tr.debugExternalPower, _yes(st.externalPower)),
      _row(tr.debugSleepy, _yes(st.sleepy)),
      _row(tr.debugCarried, _yes(st.carried)),
      _row(tr.debugMapPoint, _yes(st.mapPoint)),
      _row(tr.debugPlaced, _yes(st.placed)),
      _row(
        tr.debugFirstRotation,
        st.booting
            ? tr.debugRotationRunning(clock(app.session.bootLeftS))
            : tr.debugRotationOver,
      ),
      _row(tr.debugAnchorless, _yes(st.anchorless)),
      _row(tr.debugRelaysKnown, '${st.relays}'),
      _row(tr.debugNeighboursHeard, '${st.heard}'),
      _row(
        tr.debugAddressPosition,
        st.placed ? '${st.address}' : tr.debugNoValue,
      ),
      _row(
        tr.debugBattery,
        st.batteryMv == 0 ? tr.debugBatteryNone : tr.millivolts(st.batteryMv),
      ),
      _row(tr.debugInbox, '${st.inbox}'),
      _row(tr.gpsSignal, st.gpsCn0 == 0 ? tr.noReading : tr.dbHz(st.gpsCn0)),
    ];
  }
}
