// Settings: the node to connect to, the person's name, the node's map
// point, and the fake node.

import 'package:flutter/material.dart';
import 'package:geolocator_platform_interface/geolocator_platform_interface.dart'
    show GeolocatorPlatform, LocationPermission;

import '../app.dart';
import '../ble_link.dart';
import '../link.dart';
import 'common.dart';

class SettingsScreen extends StatefulWidget {
  final AppModel app;
  const SettingsScreen(this.app, {super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  final lat = TextEditingController();
  final lon = TextEditingController();

  AppModel get app => widget.app;

  @override
  void dispose() {
    lat.dispose();
    lon.dispose();
    if (app.scanning) app.stopScan();
    super.dispose();
  }

  Future<void> _try(Future<void> Function() f, String failed) async {
    try {
      await f();
    } catch (e) {
      if (mounted) say(context, '$failed: $e');
    }
  }

  Future<void> _phoneLocation() async {
    final g = GeolocatorPlatform.instance;
    try {
      var p = await g.checkPermission();
      if (p == LocationPermission.denied) p = await g.requestPermission();
      if (p == LocationPermission.denied ||
          p == LocationPermission.deniedForever) {
        if (mounted) say(context, 'Location permission denied');
        return;
      }
      final here = await g.getCurrentPosition();
      lat.text = here.latitude.toStringAsFixed(6);
      lon.text = here.longitude.toStringAsFixed(6);
    } catch (e) {
      if (mounted) say(context, 'No location: $e');
    }
  }

  Future<void> _setMapPoint() async {
    final la = double.tryParse(lat.text.trim());
    final lo = double.tryParse(lon.text.trim());
    if (la == null || lo == null || la.abs() > 90 || lo.abs() > 180) {
      say(context, 'Latitude and longitude in degrees, please');
      return;
    }
    await app.session.setMapPoint(Position.fromDegrees(la, lo));
  }

  @override
  Widget build(BuildContext context) => Watch(
    app,
    (context) => Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          NoticeCard(app),
          ..._connection(),
          _heading('My name'),
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: Text(
              app.contacts.myName.isEmpty ? 'not set' : app.contacts.myName,
            ),
            subtitle: const Text('On your card, for others to see'),
            trailing: IconButton(
              tooltip: 'Change',
              icon: const Icon(Icons.edit),
              onPressed: () async {
                final n = await askText(
                  context,
                  'My name',
                  app.contacts.myName,
                );
                if (n != null) await app.contacts.setMyName(n);
              },
            ),
          ),
          ..._mapPoint(),
        ],
      ),
    ),
  );

  Widget _heading(String s) => Padding(
    padding: const EdgeInsets.only(top: 24, bottom: 8),
    child: Text(s, style: Theme.of(context).textTheme.titleLarge),
  );

  List<Widget> _connection() {
    final s = app.session;
    return [
      _heading('Node'),
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Fake node'),
        subtitle: const Text(
          'A node in the app, for trying it without a board',
        ),
        value: app.fake,
        onChanged: s.connected ? null : app.setFake,
      ),
      if (s.connected)
        ListTile(
          contentPadding: EdgeInsets.zero,
          title: Text(nodeName(s.node!.id)),
          subtitle: Text('Connected, MTU ${app.mtu ?? '?'}'),
          trailing: OutlinedButton(
            onPressed: s.disconnect,
            child: const Text('Disconnect'),
          ),
        )
      else if (app.fake)
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(
            onPressed: () => _try(app.connectFake, 'Connecting failed'),
            child: const Text('Connect'),
          ),
        )
      else ...[
        Row(
          children: [
            FilledButton(
              onPressed: app.scanning || app.connecting
                  ? app.stopScan
                  : () => _try(app.scan, 'Scan failed'),
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
        for (final FoundNode n in app.found.values)
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: Text(n.name),
            subtitle: Text('${n.deviceId}, ${n.rssi ?? '?'} dBm'),
            onTap: app.connecting
                ? null
                : () => _try(() => app.connect(n), 'Connecting failed'),
          ),
      ],
    ];
  }

  List<Widget> _mapPoint() {
    final s = app.session;
    if (!s.connected) return [];
    final st = s.status;
    return [
      _heading('Map point'),
      Text(
        st == null || !st.placed
            ? 'The node has no position.'
            : st.mapPoint
            ? 'The node\'s position is its map point, ${st.address}.'
            : 'The node\'s position is ${st.address}, not from a map point.',
      ),
      Row(
        children: [
          Expanded(child: _degrees(lat, 'Latitude')),
          const SizedBox(width: 16),
          Expanded(child: _degrees(lon, 'Longitude')),
        ],
      ),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          OutlinedButton(
            onPressed: _phoneLocation,
            child: const Text('Phone location'),
          ),
          FilledButton(onPressed: _setMapPoint, child: const Text('Set')),
          OutlinedButton(
            onPressed: () => s.setMapPoint(null),
            child: const Text('Clear'),
          ),
        ],
      ),
    ];
  }

  Widget _degrees(TextEditingController c, String label) => TextField(
    controller: c,
    decoration: InputDecoration(labelText: label),
    keyboardType: const TextInputType.numberWithOptions(
      signed: true,
      decimal: true,
    ),
  );
}
