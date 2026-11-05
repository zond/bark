// One screen, in sections: connection, node, status, map point, send,
// inbox.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:geolocator_platform_interface/geolocator_platform_interface.dart'
    show GeolocatorPlatform, LocationPermission;

import 'ble_link.dart';
import 'fake_node.dart';
import 'link.dart';
import 'payload.dart';
import 'session.dart';
import 'store.dart';

void main() => runApp(const BarkApp());

class BarkApp extends StatelessWidget {
  const BarkApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'bark',
    theme: ThemeData(colorSchemeSeed: Colors.brown),
    darkTheme: ThemeData(
      colorSchemeSeed: Colors.brown,
      brightness: Brightness.dark,
    ),
    home: const Home(),
  );
}

class Home extends StatefulWidget {
  const Home({super.key});
  @override
  State<Home> createState() => _HomeState();
}

class _HomeState extends State<Home> {
  final session = Session(store: FileStore(), now: DateTime.now);
  final Map<String, FoundNode> found = {};
  bool fake = false;
  FakeNode? fakeNode;
  bool scanning = false;
  bool connecting = false;
  int? mtu;
  late final Timer ticker;

  final dst = TextEditingController();
  final text = TextEditingController();
  final lat = TextEditingController();
  final lon = TextEditingController();

  @override
  void initState() {
    super.initState();
    session.addListener(() => setState(() {}));
    session.load();
    // The countdowns move without a word from the node.
    ticker = Timer.periodic(const Duration(seconds: 1), (_) => setState(() {}));
  }

  @override
  void dispose() {
    ticker.cancel();
    fakeNode?.dispose();
    session.dispose();
    super.dispose();
  }

  void say(String s) =>
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s)));

  Future<void> scan() async {
    setState(() {
      found.clear();
      scanning = true;
    });
    try {
      await startScan((n) => setState(() => found[n.deviceId] = n));
    } catch (e) {
      say('Scan failed: $e');
      setState(() => scanning = false);
    }
  }

  Future<void> stop() async {
    await stopScan();
    setState(() => scanning = false);
  }

  Future<void> connect(FoundNode n) async {
    await stop();
    setState(() => connecting = true);
    try {
      final (link, m) = await BleLink.connect(n.deviceId);
      mtu = m;
      session.senderOf = (_) => null;
      await session.attach(link);
    } catch (e) {
      say('Connecting failed: $e');
    } finally {
      setState(() => connecting = false);
    }
  }

  Future<void> connectFake() async {
    final f = fakeNode ??= FakeNode(now: DateTime.now);
    mtu = attMtu;
    session.senderOf = f.senderOf;
    await session.attach(f.connect());
  }

  Future<void> phoneLocation() async {
    final g = GeolocatorPlatform.instance;
    try {
      var p = await g.checkPermission();
      if (p == LocationPermission.denied) p = await g.requestPermission();
      if (p == LocationPermission.denied ||
          p == LocationPermission.deniedForever) {
        say('Location permission denied');
        return;
      }
      final here = await g.getCurrentPosition();
      lat.text = here.latitude.toStringAsFixed(6);
      lon.text = here.longitude.toStringAsFixed(6);
    } catch (e) {
      say('No location: $e');
    }
  }

  Future<void> setMapPoint() async {
    final la = double.tryParse(lat.text.trim());
    final lo = double.tryParse(lon.text.trim());
    if (la == null || lo == null || la.abs() > 90 || lo.abs() > 180) {
      say('Latitude and longitude in degrees, please');
      return;
    }
    await session.setMapPoint(Position.fromDegrees(la, lo));
  }

  Future<void> queue() async {
    final a = Address.parse(dst.text);
    if (a == null) {
      say('The address is ${Address.len * 2} hex digits');
      return;
    }
    try {
      await session.queue(a, text.text);
      text.clear();
    } on Malformed {
      say('The text is longer than $maxTextBytes bytes');
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('bark')),
    body: ListView(
      padding: const EdgeInsets.all(16),
      children: [
        if (session.notice != null)
          Card(
            color: Theme.of(context).colorScheme.errorContainer,
            child: ListTile(
              title: Text(session.notice!),
              trailing: IconButton(
                icon: const Icon(Icons.close),
                onPressed: () => setState(() => session.notice = null),
              ),
            ),
          ),
        ..._connection(),
        if (session.connected) ...[..._node(), ..._status(), ..._mapPoint()],
        ..._send(),
        ..._inbox(),
      ],
    ),
  );

  Widget _heading(String s) => Padding(
    padding: const EdgeInsets.only(top: 24, bottom: 8),
    child: Text(s, style: Theme.of(context).textTheme.titleLarge),
  );

  List<Widget> _connection() => [
    _heading('Connection'),
    SwitchListTile(
      title: const Text('Fake node'),
      subtitle: const Text('A node in the app, for trying it without a board'),
      value: fake,
      onChanged: session.connected
          ? null
          : (v) => setState(() {
              fake = v;
              if (!v) {
                fakeNode?.dispose();
                fakeNode = null;
              }
            }),
    ),
    if (session.connected)
      ListTile(
        title: Text(nodeName(session.node!.id)),
        subtitle: Text('Connected, MTU ${mtu ?? '?'}'),
        trailing: OutlinedButton(
          onPressed: session.disconnect,
          child: const Text('Disconnect'),
        ),
      )
    else if (fake)
      FilledButton(onPressed: connectFake, child: const Text('Connect'))
    else ...[
      Row(
        children: [
          FilledButton(
            onPressed: scanning || connecting ? stop : scan,
            child: Text(scanning ? 'Stop' : 'Scan'),
          ),
          const SizedBox(width: 16),
          if (scanning || connecting)
            const SizedBox.square(
              dimension: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
        ],
      ),
      for (final n in found.values)
        ListTile(
          title: Text(n.name),
          subtitle: Text('${n.deviceId}, ${n.rssi ?? '?'} dBm'),
          onTap: connecting ? null : () => connect(n),
        ),
    ],
  ];

  List<Widget> _node() {
    final n = session.node!;
    final a = session.address;
    return [
      _heading('Node'),
      _row('Link version', '${n.version}'),
      _row('Firmware image', '${n.image}'),
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
                  say('Address copied');
                },
              ),
      ),
    ];
  }

  Widget _row(String k, String v) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 2),
    child: Row(
      children: [
        Expanded(child: Text(k)),
        Text(v, style: const TextStyle(fontWeight: FontWeight.bold)),
      ],
    ),
  );

  String _yes(bool b) => b ? 'yes' : 'no';

  String _clock(int s) => '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';

  List<Widget> _status() {
    final s = session.status;
    if (s == null) return [_heading('Status'), const Text('Not read yet')];
    return [
      _heading('Status'),
      _row('Ready to take a message', _yes(s.ready)),
      _row('External power', _yes(s.externalPower)),
      _row('Sleepy', _yes(s.sleepy)),
      _row('Carried', _yes(s.carried)),
      _row('Position from a map point', _yes(s.mapPoint)),
      _row('Placed', _yes(s.placed)),
      _row(
        'First rotation',
        s.booting ? 'running, ${_clock(session.bootLeftS)} to go' : 'over',
      ),
      _row('Anchorless', _yes(s.anchorless)),
      _row('Relays known', '${s.relays}'),
      _row('Address position', s.placed ? '${s.address}' : 'none'),
      _row(
        'Battery',
        s.batteryMv == 0 ? 'not read or on USB' : '${s.batteryMv} mV',
      ),
      _row('Messages in the inbox', '${s.inbox}'),
    ];
  }

  List<Widget> _mapPoint() => [
    _heading('Map point'),
    Row(
      children: [
        Expanded(
          child: TextField(
            controller: lat,
            decoration: const InputDecoration(labelText: 'Latitude'),
            keyboardType: const TextInputType.numberWithOptions(
              signed: true,
              decimal: true,
            ),
          ),
        ),
        const SizedBox(width: 16),
        Expanded(
          child: TextField(
            controller: lon,
            decoration: const InputDecoration(labelText: 'Longitude'),
            keyboardType: const TextInputType.numberWithOptions(
              signed: true,
              decimal: true,
            ),
          ),
        ),
      ],
    ),
    const SizedBox(height: 8),
    Wrap(
      spacing: 8,
      children: [
        OutlinedButton(
          onPressed: phoneLocation,
          child: const Text('Phone location'),
        ),
        FilledButton(onPressed: setMapPoint, child: const Text('Set')),
        OutlinedButton(
          onPressed: () => session.setMapPoint(null),
          child: const Text('Clear'),
        ),
      ],
    ),
  ];

  List<Widget> _send() => [
    _heading('Send'),
    TextField(
      controller: dst,
      decoration: const InputDecoration(
        labelText: 'To: address, ${Address.len * 2} hex digits',
      ),
    ),
    TextField(
      controller: text,
      decoration: const InputDecoration(labelText: 'Text, not encrypted'),
      maxLines: null,
    ),
    const SizedBox(height: 8),
    Align(
      alignment: Alignment.centerLeft,
      child: FilledButton(onPressed: queue, child: const Text('Queue')),
    ),
    for (final o in session.outbox.reversed) _outgoing(o),
  ];

  Widget _outgoing(Outgoing o) {
    final wait = session.waitLeftS(o);
    final state = switch (o.state) {
      OutState.queued => 'queued',
      OutState.writing => 'offering to the node',
      OutState.taken => 'taken by the node',
      OutState.givenBack =>
        wait > 0 ? 'given back, wait ${_clock(wait)}' : 'given back, wait over',
      OutState.refused => 'refused: ${o.error}',
    };
    return Card(
      child: ListTile(
        title: Text(o.text),
        subtitle: Text('${msgIdHex(o.id)} to ${o.dst}\n$state'),
        isThreeLine: true,
        trailing: Wrap(
          children: [
            if (o.state == OutState.givenBack)
              IconButton(
                tooltip: 'Send again',
                icon: const Icon(Icons.replay),
                onPressed: wait > 0 ? null : () => session.offerAgain(o),
              ),
            if (o.state != OutState.writing)
              IconButton(
                tooltip: 'Drop',
                icon: const Icon(Icons.delete_outline),
                onPressed: () => session.drop(o),
              ),
          ],
        ),
      ),
    );
  }

  List<Widget> _inbox() => [
    _heading('Received'),
    if (session.received.isEmpty) const Text('Nothing yet'),
    for (final r in session.received.reversed)
      Card(
        child: ListTile(
          title: Text(r.text),
          subtitle: Text('${msgIdHex(r.id)}, ${r.at.toLocal()}'),
        ),
      ),
  ];
}
