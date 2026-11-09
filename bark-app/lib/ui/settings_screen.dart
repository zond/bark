// Settings: the node to connect to and the phone paired with it, the
// fake node, whether the node serves phones without pairing, the node's
// firmware, the battery exemption
// that keeps the link with the screen off, the person's name, their home
// node, their backup words, and the node's map point.

import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';
import 'package:geolocator_platform_interface/geolocator_platform_interface.dart'
    show GeolocatorPlatform, LocationPermission;

import '../app.dart';
import '../app_layer.dart';
import '../ble_link.dart';
import '../link.dart';
import '../node_link.dart';
import 'common.dart';
import 'debug_screen.dart';
import 'update_screen.dart';

class SettingsScreen extends StatefulWidget {
  final AppModel app;

  /// Opened to act on the backup: scrolled to it, which shows tinted for
  /// a moment and is announced.
  final bool toBackup;
  const SettingsScreen(this.app, {this.toBackup = false, super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  final lat = TextEditingController();
  final lon = TextEditingController();

  AppModel get app => widget.app;

  /// Whether Android exempts the app from battery optimisation, read again
  /// when the person comes back from the system's dialog.
  bool? _exempt;
  late final AppLifecycleListener _life;

  /// The backup's place in the list, to scroll to.
  final _backupKey = GlobalKey();

  /// The backup shows tinted, fading once it is in view.
  bool _backupLit = false;

  /// How long the tint takes to fade: long enough to be seen after the
  /// scroll, short enough not to stay in the way.
  static const _fade = Duration(milliseconds: 1500);

  @override
  void initState() {
    super.initState();
    _readExempt();
    _life = AppLifecycleListener(onResume: _readExempt);
    if (widget.toBackup) {
      _backupLit = true;
      WidgetsBinding.instance.addPostFrameCallback((_) => _showBackup());
    }
  }

  Future<void> _showBackup() async {
    final c = _backupKey.currentContext;
    if (c == null) return;
    await Scrollable.ensureVisible(
      c,
      duration: const Duration(milliseconds: 300),
      alignment: 0.1,
    );
    if (!mounted) return;
    SemanticsService.sendAnnouncement(
      View.of(context),
      'Backup: write down your 12 words',
      TextDirection.ltr,
    );
    setState(() => _backupLit = false);
  }

  Future<void> _readExempt() async {
    final e = await app.notifier.batteryExempt();
    if (mounted) setState(() => _exempt = e);
  }

  @override
  void dispose() {
    _life.dispose();
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

  /// A connection asked for here has not ended: a second tap meanwhile
  /// does nothing, as it would close the link the first is making.
  bool _connectAsked = false;

  /// Connects by `f`, at the first connection after asking to notify.
  Future<void> _connect(Future<void> Function() f) async {
    if (_connectAsked) return;
    _connectAsked = true;
    try {
      await _connectOnce(f);
    } finally {
      _connectAsked = false;
    }
  }

  Future<void> _connectOnce(Future<void> Function() f) async {
    await app.askToNotify(
      () async =>
          await showDialog<bool>(
            context: context,
            builder: (context) => AlertDialog(
              title: const Text('Notifications'),
              content: const Text(
                'bark tells you when a text comes, and shows that it keeps '
                'the link to the node while the screen is off. Android asks '
                'you next.',
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.pop(context, false),
                  child: const Text('Not now'),
                ),
                FilledButton(
                  onPressed: () => Navigator.pop(context, true),
                  child: const Text('Continue'),
                ),
              ],
            ),
          ) ??
          false,
    );
    try {
      await f();
    } on NotOurs catch (e) {
      if (mounted) await _notOurs(e);
    } catch (e) {
      if (mounted) say(context, 'Connecting failed: $e');
    }
  }

  /// The node no longer holds this phone's bond: pair again, by the code
  /// on its screen, or leave it.
  Future<void> _notOurs(NotOurs e) async {
    final again = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('This node is no longer yours'),
        content: Text(
          'Another phone has paired with ${shownName(e.node)} since, and the node '
          'keeps one phone. To make it yours again, pair again at the '
          'node: it shows a six-digit code on its screen, and the phone '
          'asks for it.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Not now'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Pair again'),
          ),
        ],
      ),
    );
    final pair = app.pairAgain;
    if (again != true || pair == null) return;
    await _connectOnce(pair);
  }

  /// The phone's position, after asking for location if needed; null,
  /// with the person told why, without one.
  Future<Position?> _phonePosition() async {
    final g = GeolocatorPlatform.instance;
    try {
      var p = await g.checkPermission();
      if (p == LocationPermission.denied) p = await g.requestPermission();
      if (p == LocationPermission.denied ||
          p == LocationPermission.deniedForever) {
        if (mounted) say(context, 'Location permission denied');
        return null;
      }
      final here = await g.getCurrentPosition();
      return Position.fromDegrees(here.latitude, here.longitude);
    } catch (e) {
      if (mounted) say(context, 'No location: $e');
      return null;
    }
  }

  Future<void> _phoneLocation() async {
    final p = await _phonePosition();
    if (p == null) return;
    lat.text = p.latDeg.toStringAsFixed(6);
    lon.text = p.lonDeg.toStringAsFixed(6);
  }

  /// Gives the node the phone's position as its place, so that it has an
  /// address to be a home at.
  Future<void> _placeHere() async {
    final p = await _phonePosition();
    if (p != null) await app.session.setMapPoint(p);
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
      body: SafeArea(
        top: false,
        // Built whole, not lazily, so that the backup can be scrolled to.
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              NoticeCard(app),
              ..._connection(),
              ..._public(),
              ..._home(),
              ..._firmware(),
              ..._battery(),
              _heading('My name'),
              ListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(app.me.name.isEmpty ? 'not set' : app.me.name),
                subtitle: const Text('On your card, for others to see'),
                trailing: IconButton(
                  tooltip: 'Change',
                  icon: const Icon(Icons.edit),
                  onPressed: () => askMyName(context, app),
                ),
              ),
              AnimatedContainer(
                key: _backupKey,
                duration: _fade,
                curve: Curves.easeOut,
                decoration: BoxDecoration(
                  color: _backupLit
                      ? Theme.of(context).colorScheme.primaryContainer
                      : Colors.transparent,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: _backup(),
                ),
              ),
              ..._mapPoint(),
              const SizedBox(height: 24),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: const Text('Try without a node (demo)'),
                subtitle: const Text(
                  'A node in the app with an invented network; nothing leaves '
                  'this phone',
                ),
                value: app.fake,
                onChanged: app.session.connected ? null : app.setFake,
              ),
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.bug_report_outlined),
                title: const Text('Debug'),
                subtitle: const Text(
                  'Everything the link says, for a bug report',
                ),
                onTap: () => Navigator.of(context)
                    .push(MaterialPageRoute(builder: (_) => DebugScreen(app))),
              ),
            ],
          ),
        ),
      ),
    ),
  );

  Widget _heading(String s) => Padding(
    padding: const EdgeInsets.only(top: 24, bottom: 8),
    child: Text(s, style: Theme.of(context).textTheme.titleLarge),
  );

  /// What a connection being made is doing, for the line under the
  /// node.
  static String _linking(Linking l) => switch (l) {
    Linking.none => '',
    Linking.connecting => 'Connecting…',
    Linking.pairing =>
      'Pairing: type the six-digit code the node\'s screen shows into the '
          'phone\'s dialog. After it, the phone takes some seconds to '
          'finish.',
    Linking.paired || Linking.attaching => 'Paired, connecting…',
  };

  List<Widget> _connection() {
    final s = app.session;
    return [
      _heading('Node'),
      if (s.connected)
        ListTile(
          contentPadding: EdgeInsets.zero,
          title: Text(nodeLabel(s.node!.id)),
          subtitle: Text(
            s.guest
                ? 'Connected as a guest: a public node, someone else\'s, '
                      'which passes on your texts without pairing. '
                      '${health(s)}'
                : 'Connected and paired. ${health(s)}',
          ),
          trailing: OutlinedButton(
            onPressed: app.disconnect,
            child: const Text('Disconnect'),
          ),
        )
      else ...[
        Text(app.connecting ? _linking(app.linking) : pairedLine(s)),
        const SizedBox(height: 8),
      ],
      if (app.fake && !s.connected)
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(
            onPressed: () => _connect(app.connectFake),
            child: const Text('Connect'),
          ),
        )
      else if (!app.fake) ...[
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
        // The connected node's row does nothing; another's connects to
        // it, closing the link to this one first: a public node not the
        // phone's as a guest, which **Pair** makes the phone's instead.
        for (final FoundNode n in app.found.values) _found(n),
        ..._looking(),
      ],
    ];
  }

  Widget _found(FoundNode n) {
    final guest = n.public && n.name != app.pairedNode;
    final busy = app.connecting || app.connectedTo(n.deviceId);
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(n.public ? Icons.storefront : Icons.router),
      title: Text(shownName(n.name)),
      subtitle: Text(
        [
          if (app.connectedTo(n.deviceId)) 'Connected',
          if (n.public) 'public: serves phones without pairing',
          '${n.rssi ?? '?'} dBm',
        ].join(', '),
      ),
      onTap: busy
          ? null
          : () => guest ? _yours(n) : _connect(() => app.connect(n)),
    );
  }

  /// A public node not paired with: the person says whether it is theirs.
  Future<void> _yours(FoundNode n) async {
    final mine = await askIsYours(context, n);
    if (mine == null || !mounted) return;
    await _connect(() => mine ? app.connect(n) : app.connectGuest(n));
  }

  /// The owner's switch of whether the node serves phones that have not
  /// paired with it, from link version 7.
  List<Widget> _public() {
    final s = app.session;
    final p = s.nodePublic;
    if (!s.connected || s.guest || p == null) return [];
    return [
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Public: serve phones without pairing'),
        subtitle: const Text(
          'On unless you turn it off: phones near the node that have not '
          'paired with it can send and receive their texts through it, as '
          'at a shop\'s or a café\'s node. They cannot change its '
          'settings, and your texts stay yours. Off, the node is yours '
          'alone.',
        ),
        value: p,
        onChanged: (v) => _try(() => s.writePublic(v), 'Setting Public failed'),
      ),
    ];
  }

  /// The search for the paired node, which the person can stop until
  /// they connect.
  List<Widget> _looking() {
    final l = lookingLine(app);
    if (l == null) return [];
    return [
      const SizedBox(height: 8),
      Row(
        children: [
          Expanded(child: Text(l)),
          TextButton(
            onPressed: app.stopLooking,
            child: const Text('Stop looking'),
          ),
        ],
      ),
    ];
  }

  /// The connected node's firmware, and its update over the link.
  List<Widget> _firmware() {
    final n = app.session.node;
    if (n == null || app.session.guest) return [];
    return [
      _heading('Firmware'),
      Text(
        n.version < updateVersion
            ? 'The node runs version ${n.image}, from before updates over '
                  'Bluetooth: update it by USB once.'
            : 'The node runs version ${n.image}.',
      ),
      const SizedBox(height: 8),
      Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton(
          onPressed: n.version < updateVersion
              ? null
              : () => Navigator.of(context)
                    .push(MaterialPageRoute(builder: (_) => UpdateScreen(app))),
          child: const Text('Update firmware'),
        ),
      ),
    ];
  }

  List<Widget> _battery() {
    final e = _exempt;
    if (e == null) return [];
    return [
      _heading('Battery'),
      Text(
        e
            ? 'bark is exempt from battery optimisation: Android lets it '
                  'keep the link to the node while the screen is off.'
            : 'bark is not exempt from battery optimisation: with the '
                  'screen off, some phones close its link to the node after '
                  'some hours.',
      ),
      if (!e) ...[
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(
            onPressed: app.notifier.askBatteryExempt,
            child: const Text('Exempt bark'),
          ),
        ),
      ],
    ];
  }

  List<Widget> _home() {
    final s = app.session;
    final home = app.me.home;
    final here = s.address;
    final st = s.status;
    final atHome = app.atHome;
    // A guest can neither make the node its home nor clear its key.
    final owned = !s.guest;
    // A home is a fixed relay, so that its address stays (spec "Mailbox").
    final homeable = st != null && !st.sleepy && !st.carried;
    return [
      _heading('Home node'),
      Text(
        home == null
            ? 'No home node: your card is temporary, naming the node you '
                  'use now, which keeps your mail only while you come back '
                  'to it.'
            : 'Your home node is at $home. Your card carries its address, '
                  'and it holds your mailbox key. "Fetch my mail", the '
                  'inbox button on the first screen, brings your mail from '
                  'it to the node you are connected to.',
      ),
      if (owned && s.connected && here == null && st != null) ...[
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text('This node has no place yet.'),
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: OutlinedButton.icon(
            icon: const Icon(Icons.my_location),
            label: const Text('Use this phone\'s location'),
            onPressed: _placeHere,
          ),
        ),
      ],
      if (here != null && !atHome && !homeable)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'A home must be a node that stays on power at a fixed place.',
          ),
        ),
      if (app.holdsOtherKey)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'This node holds a mailbox key: it is someone\'s home, perhaps '
            'a former one of yours.',
          ),
        ),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          if (owned && here != null && !atHome)
            FilledButton(
              onPressed: homeable ? () => _setHome(here) : null,
              child: const Text('Make this node my home'),
            ),
          if (home != null)
            OutlinedButton(
              onPressed: () => _setHome(null),
              child: const Text('No home node'),
            ),
          if (owned && app.holdsOtherKey)
            OutlinedButton(
              onPressed: () => _try(() async {
                final r = await app.clearKey();
                if (mounted) say(context, r);
              }, 'Clearing the key failed'),
              child: const Text('Clear its mailbox key'),
            ),
        ],
      ),
      SendCardLine(app, padding: EdgeInsets.zero),
    ];
  }

  Future<void> _setHome(Address? a) async {
    final n = app.contacts.list.length;
    // The card carries the home; the new card goes out only if it
    // changes.
    final changes = a?.toString() != app.me.home?.toString();
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(a == null ? 'No home node' : 'Make this node my home'),
        content: Text(
          !changes
              ? 'Your card stays as it is, so no card is sent.'
              : n == 0
              ? 'Your card changes.'
              : n == 1
              ? 'Your card changes, and your new card goes to '
                    'your contact.'
              : 'Your card changes, and your new card goes to '
                    'your $n contacts, one message each.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Go ahead'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    await _try(() async {
      final r = await (a == null ? app.noHome() : app.makeHome(a));
      if (mounted) say(context, r);
    }, 'Setting the home failed');
  }

  List<Widget> _backup() => [
    _heading('Backup'),
    Text(
      app.me.backedUp
          ? 'Your identity comes from 12 words. With them it can be '
                'restored on another phone.'
          : 'Write down these 12 words: they are your identity. Without '
                'them, a lost phone loses it.',
    ),
    const SizedBox(height: 8),
    Wrap(
      spacing: 8,
      children: [
        FilledButton(
          onPressed: () => _showWords(),
          child: const Text('Show my words'),
        ),
        OutlinedButton(
          onPressed: _restore,
          child: const Text('Restore from words'),
        ),
      ],
    ),
  ];

  Future<void> _showWords() async {
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('My 12 words'),
        content: SelectableText(
          app.me.phrase.join(' '),
          style: const TextStyle(fontSize: 18),
        ),
        actions: [
          FilledButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('I wrote them down'),
          ),
        ],
      ),
    );
    await app.me.setBackedUp();
  }

  Future<void> _restore() async {
    final go = await confirm(
      context,
      'Restore from words?',
      'This phone takes the identity of the words in place of its own. '
          'Contacts who hold the card of the identity it has now cannot '
          'reach the new one until they get its card.',
      'Restore',
    );
    if (!go || !mounted) return;
    final w = await askText(context, 'Your 12 words', '');
    if (w == null || w.isEmpty) return;
    final seed = seedOfPhrase(w);
    if (seed == null) {
      if (mounted) say(context, 'Those are not 12 words of a backup');
      return;
    }
    await app.me.restore(seed);
    if (mounted) say(context, 'Identity restored');
  }

  List<Widget> _mapPoint() {
    final s = app.session;
    if (!s.connected || s.guest) return [];
    final st = s.status;
    return [
      _heading('Node\'s place'),
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
