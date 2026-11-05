// Everything the link says, raw: Node, Status, Given back, the message
// IDs and payloads, and the log of the link's events.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app.dart';
import '../link.dart';
import '../session.dart';
import 'common.dart';

class DebugScreen extends StatelessWidget {
  final AppModel app;
  const DebugScreen(this.app, {super.key});

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    Widget heading(String t) => Padding(
      padding: const EdgeInsets.only(top: 24, bottom: 8),
      child: Text(t, style: Theme.of(context).textTheme.titleLarge),
    );
    return Scaffold(
      appBar: AppBar(title: const Text('Debug')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          NoticeCard(app),
          heading('Me'),
          ..._me(),
          heading('Node'),
          ..._node(context),
          heading('Status'),
          ..._status(),
          heading('Given back'),
          _row(
            'Value',
            !s.connected
                ? 'not connected'
                : switch (s.givenBack) {
                    null => 'empty',
                    final g => '${msgIdHex(g.msgId)}, wait ${g.waitS} s',
                  },
          ),
          heading('Sent'),
          if (s.outbox.isEmpty) const Text('None'),
          for (final o in s.outbox.reversed)
            _raw(
              '${msgIdHex(o.id)} to ${o.dst}',
              '${o.kind.name} ${o.sent} to ${_who(o.to)}, '
                  '${o.state.name}${_wait(s, o)}, queued ${o.at.toLocal()}\n'
                  '${hex(o.payload)}',
            ),
          heading('Received'),
          if (s.received.isEmpty) const Text('None'),
          for (final r in s.received.reversed)
            _raw(
              '${msgIdHex(r.id)} from ${r.from == null ? 'nobody: did not open' : _who(r.from)}',
              'sent ${r.sent}${r.mailbox ? ', from the mailbox' : ''}, '
                  '${r.at.toLocal()}\n${hex(r.payload)}',
            ),
          heading('Log'),
          if (s.log.isEmpty) const Text('Nothing yet'),
          for (final l in s.log.reversed)
            Text(
              '${l.at.toLocal().toIso8601String().substring(11, 23)} '
              '${l.text}',
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
            ),
        ],
      ),
    );
  });

  /// A contact's name and the start of its identity.
  String _who(String? key) {
    if (key == null) return 'the mailbox';
    final c = app.contacts.byKey(key);
    return '${c?.name ?? 'not a contact'} (${key.substring(0, 8)}…)';
  }

  List<Widget> _me() {
    final me = app.me;
    return [
      _row('Identity', '${me.key.substring(0, 16)}…'),
      _row('Card address', '${me.cardAddress ?? 'none yet'}'),
      _row('Home node', '${me.home ?? 'none'}'),
      _row('Mailbox: have every item up to', '${me.since}'),
      _row('Mailbox: items above that', '${me.above.toList()..sort()}'),
      _row('Fetch rounds', '${me.rounds}'),
      _row('Last fetched to', '${me.fetchedTo ?? 'never'}'),
      _row('Last sent', '${me.lastSent}'),
    ];
  }

  String _wait(Session s, Outgoing o) =>
      o.state == OutState.givenBack ? ', ${s.waitLeftS(o)} s left' : '';

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

  String _yes(bool b) => b ? 'yes' : 'no';

  List<Widget> _node(BuildContext context) {
    final s = app.session;
    final n = s.node;
    if (n == null) return [const Text('Not connected')];
    final a = s.address;
    return [
      _row('Name', nodeName(n.id)),
      _row('Node ID', hex(n.id)),
      _row('MTU', '${app.mtu ?? '?'}'),
      _row('Link version', '${n.version}'),
      _row('Firmware image', '${n.image}'),
      _row(
        'Clock',
        n.version < clockVersion
            ? 'none before link version $clockVersion'
            : switch (s.nodeTime) {
                null => 'not read',
                final t => describeTime(t),
              },
      ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Address'),
        subtitle: SelectableText(a?.toString() ?? 'none yet: no place'),
        trailing: a == null
            ? null
            : IconButton(
                icon: const Icon(Icons.copy),
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: a.toString()));
                  say(context, 'Address copied');
                },
              ),
      ),
    ];
  }

  List<Widget> _status() {
    final st = app.session.status;
    if (st == null) return [const Text('Not read')];
    return [
      _row('Ready to take a message', _yes(st.ready)),
      _row('External power', _yes(st.externalPower)),
      _row('Sleepy', _yes(st.sleepy)),
      _row('Carried', _yes(st.carried)),
      _row('Position from a map point', _yes(st.mapPoint)),
      _row('Placed', _yes(st.placed)),
      _row(
        'First rotation',
        st.booting ? 'running, ${clock(app.session.bootLeftS)} to go' : 'over',
      ),
      _row('Anchorless', _yes(st.anchorless)),
      _row('Relays known', '${st.relays}'),
      _row('Address position', st.placed ? '${st.address}' : 'none'),
      _row(
        'Battery',
        st.batteryMv == 0 ? 'not read or on USB' : '${st.batteryMv} mV',
      ),
      _row('Messages in the inbox', '${st.inbox}'),
    ];
  }
}
