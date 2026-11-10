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
import '../releases.dart';
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

  /// Connects by `f`, at the first connection after asking to notify;
  /// `pairing` a node the phone uses as a guest, which it goes on doing
  /// if `f` fails.
  Future<void> _connect(
    Future<void> Function() f, {
    bool pairing = false,
  }) async {
    if (_connectAsked) return;
    _connectAsked = true;
    try {
      await _connectOnce(f, pairing: pairing);
    } finally {
      _connectAsked = false;
    }
  }

  Future<void> _connectOnce(
    Future<void> Function() f, {
    bool pairing = false,
  }) async {
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
    } on StaleBond catch (e) {
      if (mounted) await askToForget(context, app, staleBondLine(e.node));
    } catch (e) {
      if (mounted) {
        say(
          context,
          pairing
              ? notPairedLine(e)
              : e is NotPaired
              ? '$e.'
              : 'Connecting failed: $e',
        );
      }
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
          '${shownName(e.node)} no longer knows this phone: someone paired '
          'with it removed this phone, or, on older firmware, another phone '
          'paired in its place. To make it yours again, pair again at the '
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

  /// The coordinates the phone's location last set as the node's place,
  /// for the line that confirms it.
  String? _placeSet;

  /// Gives the node the phone's location as its place at once.
  Future<void> _placeHere() async {
    final p = await _phonePosition();
    if (p == null) return;
    final la = p.latDeg.toStringAsFixed(6);
    final lo = p.lonDeg.toStringAsFixed(6);
    lat.text = la;
    lon.text = lo;
    await app.session.setMapPoint(p);
    if (mounted) setState(() => _placeSet = '$la, $lo');
  }

  /// The one action that sets the node's place from the phone, the same
  /// wherever it shows, with the line that says what it set.
  List<Widget> _placeHereButton() => [
    Align(
      alignment: Alignment.centerLeft,
      child: OutlinedButton.icon(
        icon: const Icon(Icons.my_location),
        label: const Text('Use this phone\'s location'),
        onPressed: _placeHere,
      ),
    ),
    if (_placeSet != null)
      Text(
        'Place set to $_placeSet.',
        style: Theme.of(context).textTheme.bodySmall,
      ),
  ];

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
              StaleBondLine(app),
              ..._connection(),
              ..._public(),
              ..._phones(),
              ..._home(),
              ..._updates(),
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
      ..._paired(),
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
        // phone's as a guest, or paired with at once by its Pair.
        for (final FoundNode n in app.found.values) _found(n),
        if (app.noneFound) ...[
          const SizedBox(height: 8),
          const Text('No node found nearby.'),
          Text(
            'Switch a node on, or come closer to it.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ],
    ];
  }

  /// The nodes this phone is paired with, the home first, each with how
  /// it stands, and Unpair on the one connected before link version 13,
  /// where the phone's own row of the node's phones has it ([_phones]);
  /// a tap on another connects to it as its owner (spec "Phone link",
  /// Pairing).
  List<Widget> _paired() {
    final s = app.session;
    final home = app.homeName;
    final names = [
      if (s.pairedWith.contains(home)) home!,
      for (final n in s.pairedWith)
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
        ListTile(
          contentPadding: EdgeInsets.zero,
          leading: Icon(n == home ? Icons.home_outlined : Icons.router),
          title: Text(shownName(n)),
          subtitle: switch (_pairedState(n, home: n == home, on: n == on)) {
            '' => null,
            final w => Text(w),
          },
          onTap: n == on || s.pairingLost.contains(n) || app.connecting
              ? null
              : () => _connect(() => app.connectPaired(n)),
          trailing:
              n == on &&
                  s.node!.version >= unpairVersion &&
                  s.node!.version < phonesVersion
              ? TextButton(onPressed: _askUnpair, child: const Text('Unpair'))
              : s.pairingLost.contains(n)
              ? TextButton(
                  onPressed: () => _askForget(n, home: n == home),
                  child: const Text('Forget'),
                )
              : null,
        ),
    ];
  }

  /// How a paired node stands, in words. A paired node the search hears
  /// it connects to, so one not connected while it looks is out of reach;
  /// while the phone is on another node the search stops and cannot tell.
  String _pairedState(String n, {required bool home, required bool on}) {
    final lost = app.session.pairingLost.contains(n);
    return capital(
      [
        if (home) 'your home',
        if (on)
          'connected'
        else if (lost)
          'no longer yours: it no longer knows this phone'
        else if (app.searchOn)
          'not in reach'
        else if (app.session.stopped)
          'not looking. Tap to connect',
      ].join(', '),
    );
  }

  Widget _found(FoundNode n) {
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
            if (n.public) 'public: serves phones without pairing',
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
          : () => _connect(() => guest ? app.connectGuest(n) : app.connect(n)),
      trailing: guest
          ? TextButton(
              onPressed: app.connecting ? null : () => _askPair(n),
              child: const Text('Pair'),
            )
          : null,
    );
  }

  /// Pairs with `n`, a public node, by the code on its screen, once the
  /// person has read what pairing gives; the home is set apart.
  Future<void> _askPair(FoundNode n) async {
    final g = app.guestHere;
    if (g != null && g.found.name == n.name && g.room == 0) {
      say(context, fullNode);
      return;
    }
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Pair with ${shownName(n.name)}'),
        content: const Text(
          'Pairing makes this node one of yours: you can change its '
          'settings, place it on the map and update it from this phone, and '
          'the phone keeps a link to it while it is in reach, unless your '
          'home is in reach too. You type the six-digit code its screen '
          'shows. A node keeps up to eight phones, and those paired with it '
          'before keep theirs.\n\n'
          'Pairing does not make it your home: that is set under Home node '
          'once you are connected.',
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
    if (ok == true) await _connect(() => app.connect(n));
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

  /// The node's phones, on an owner's link from link version 13: those
  /// paired with it, and those whose home it is, a record holding both
  /// in both lists (spec "Phone link", Phones). Another's row has Remove,
  /// the person's own paired row Unpair; their own home is changed under
  /// Home node.
  List<Widget> _phones() {
    final s = app.session;
    final rs = s.phones;
    if (!s.connected || s.guest || rs == null) return [];
    final paired = rs.where((r) => r.paired).toList();
    final homes = rs.where((r) => r.home).toList();
    return [
      _heading('Phones at this node'),
      Text(
        'A node keeps up to eight phones: anyone at it can pair by the '
        'six-digit code it shows, and every phone paired with it may change '
        'its settings. Remove a phone you do not know, or one that is gone.',
        style: Theme.of(context).textTheme.bodySmall,
      ),
      if ((s.node?.room ?? 1) == 0)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            'It keeps eight now and takes no more pairings until one is '
            'removed.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
      _subheading('Phones paired with this node'),
      for (final r in paired) _phoneRow(r, home: false),
      if (homes.isNotEmpty) ...[
        _subheading('Phones with a home here'),
        for (final r in homes) _phoneRow(r, home: true),
      ],
    ];
  }

  Widget _subheading(String t) => Padding(
    padding: const EdgeInsets.only(top: 12),
    child: Text(t, style: Theme.of(context).textTheme.titleSmall),
  );

  /// A record's row in the paired list, or with `home` in the homes list.
  Widget _phoneRow(PhoneRecord r, {required bool home}) {
    final both = r.paired && r.home;
    final words = [
      if (r.mine) 'this phone',
      if (both) home ? 'paired too' : 'has a home here too',
      if (r.linked)
        'linked now'
      else
        'last linked ${lastLinked(r.day, app.now())}',
    ];
    final Widget? action;
    if (!r.mine) {
      action = TextButton(
        onPressed: () => _askRemove(r),
        child: const Text('Remove'),
      );
    } else if (!home) {
      action = TextButton(onPressed: _askUnpair, child: const Text('Unpair'));
    } else {
      // The person's own home is changed under Home node, below.
      action = null;
    }
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(home ? Icons.home_outlined : Icons.phone_android),
      title: Text(r.name.isEmpty ? 'A phone with no name' : r.name),
      subtitle: Text(capital(words.join(', '))),
      trailing: action,
    );
  }

  /// Removes another phone's pairing, home or both, after a dialog that
  /// says what follows, by the code the node's screen shows.
  Future<void> _askRemove(PhoneRecord r) async {
    final who = r.name.isEmpty
        ? 'the phone with no name'
        : '${r.name}\'s phone';
    final whoCap = capital(who);
    final parts = [
      if (r.paired)
        '$whoCap is no longer paired with the node. At its next link the '
            'node ends it, and its app says the node is no longer theirs; '
            'pairing again takes the code on the node\'s screen.',
      if (r.home)
        'The node is no longer ${r.name.isEmpty ? 'its person' : r.name}\'s '
            'home: it forgets their mailbox key, and their app says so at '
            'their next link here. Their fetches from elsewhere go '
            'unanswered.',
    ];
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(
          r.paired && r.home
              ? 'Remove $who and its home?'
              : r.paired
              ? 'Remove $who?'
              : 'Remove this home?',
        ),
        content: Text(
          '${parts.join('\n\n')}\n\nTo show that you are at the node, it '
          'draws a six-digit code on its screen, which you type here '
          'within 30 s.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Show the code'),
          ),
        ],
      ),
    );
    if (ok != true || !mounted) return;
    await _removeByCode(r);
  }

  /// Asks the node for a code, takes the six digits, and writes the
  /// removal; a wrong or late code offers a new one, as each code takes
  /// one try.
  Future<void> _removeByCode(PhoneRecord r) async {
    final s = app.session;
    while (mounted) {
      try {
        await s.askRemovalCode();
      } catch (e) {
        if (mounted) say(context, 'Asking the node for a code failed: $e');
        return;
      }
      if (!mounted) return;
      final code = await showDialog<String>(
        context: context,
        builder: (context) => const _CodeDialog(),
      );
      if (code == null || !mounted) return;
      try {
        await s.removePhone(
          r,
          bond: r.paired,
          home: r.home,
          code: int.parse(code),
        );
        if (mounted) say(context, 'Removed');
        return;
      } on AttException catch (e) {
        if (!mounted) return;
        if (e.code != AttError.refused) {
          say(
            context,
            'The node no longer holds that; the list is read again.',
          );
          return;
        }
        final again = await confirm(
          context,
          'Not removed',
          'That was not the code on the node\'s screen, or it came after '
              '30 s. Each code takes one try.',
          'New code',
        );
        if (!again) return;
      } catch (e) {
        if (mounted) say(context, 'Removing failed: $e');
        return;
      }
    }
  }

  Future<void> _askUnpair() async {
    final s = app.session;
    final name = nodeName(s.node!.id);
    final others = s.pairedWith.length > 1
        ? ' It stays paired with your other nodes.'
        : '';
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Unpair ${shownName(name)}?'),
        content: Text(
          'The node forgets this phone. Its screen then says UNPAIRED, '
          'and it is public again, serving any phone nearby. Anyone at '
          'the node can pair with it by the code on its screen.\n\n'
          'Your texts and contacts stay on this phone, and the phone '
          'forgets its own pairing with the node too.$others',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Unpair'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    await _try(() async {
      // The home is the node's mailbox key, which it keeps.
      final home = app.atHome
          ? ' It stays your home node, so texts to you still go to it.'
          : '';
      final forgotten = await app.unpair();
      if (!mounted) return;
      if (forgotten) {
        say(context, 'Unpaired: the node is public again.$home');
        return;
      }
      await askToForget(
        context,
        app,
        'Unpaired: the node is public again.$home This phone could not '
        'forget its own pairing with it: forget the node in the phone\'s '
        'Bluetooth settings, or the phone cannot use it as a guest.',
      );
    }, 'Unpairing failed');
  }

  /// Forgets `name`, a node no longer this phone's, after a dialog that
  /// says what follows; where the phone's own pairing with it may stay,
  /// says how to forget that too.
  Future<void> _askForget(String name, {required bool home}) async {
    final stays = home
        ? ' It stays your home node, so texts to you still go to it.'
        : '';
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Forget ${shownName(name)}?'),
        content: Text(
          'It no longer knows this phone, so it is no longer yours. '
          'This phone stops listing it, and forgets its own pairing with '
          'it. To make it yours again, pair at the node by the code on '
          'its screen.$stays',
        ),
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
    if (ok != true) return;
    await _try(() async {
      final gone = await app.forget(name);
      if (!mounted) return;
      switch (gone) {
        case true:
          say(context, 'Forgotten.$stays');
        case false:
          await askToForget(
            context,
            app,
            'Forgotten.$stays This phone could not forget its own pairing '
            'with it: forget the node in the phone\'s Bluetooth settings.',
          );
        case null:
          await askToForget(
            context,
            app,
            'Forgotten.$stays The phone has not heard the node since the app '
            'started, so it forgets its own pairing with it once it does. '
            'To do it now, forget the node in the phone\'s Bluetooth '
            'settings.',
          );
      }
    }, 'Forgetting failed');
  }

  /// The outcome of the last check the person asked for: null before
  /// one, else whether the release list was reached.
  bool? _reached;

  Future<void> _check() async {
    final ok = await app.releases.check(now: true);
    if (mounted) setState(() => _reached = ok);
  }

  /// Updates: the check the person asks for, with its outcome; the app's
  /// version and the way to a newer one; the node's image and the way to
  /// send it a newer one. A firmware file for the bench is picked in
  /// Debug.
  List<Widget> _updates() {
    final r = app.releases;
    final s = app.session;
    final n = s.node;
    final latest = r.latest;
    final newerApp =
        latest != null &&
        latest.apk != null &&
        r.app != 0 &&
        latest.app > r.app;
    final owner = n != null && !s.guest;
    final newerImage =
        owner &&
        latest != null &&
        latest.image != null &&
        n.image != 0 &&
        latest.firmware > n.image;
    final reached = _reached;
    return [
      _heading('Updates'),
      Row(
        children: [
          OutlinedButton(
            onPressed: r.checking || r.fetch == null ? null : _check,
            child: const Text('Check for updates'),
          ),
          const SizedBox(width: 16),
          if (r.checking)
            const SizedBox.square(
              dimension: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
        ],
      ),
      if (reached != null && !r.checking)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            reached
                ? updatesLine(r, n, guest: s.guest)
                : 'Could not reach the release list',
          ),
        ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('App'),
        subtitle: Text(
          (r.app == 0
                  ? 'A development build'
                  : newerApp
                  ? 'Version ${r.app}; version ${latest.app} is available: '
                        'tap to download it in the browser, and Android asks '
                        'before it installs it over this one'
                  : 'Version ${r.app}') +
              // A newer node works with this app, which uses what it knows
              // (roadmap D176).
              (s.nodeNewer
                  ? '. The node\'s firmware is newer than this app: it '
                        'works, and an update of the app brings the node\'s '
                        'new features.'
                  : ''),
        ),
        onTap: newerApp ? () => app.notifier.openLink(latest.apk!) : null,
      ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Node firmware'),
        subtitle: Text(
          n == null
              ? 'No node linked'
              : s.guest
              ? 'Only the phone paired with the node updates it'
              : n.version < updateVersion
              ? 'Version ${n.image}, from before updates over Bluetooth: '
                    'update it by USB once'
              : newerImage
              ? 'Version ${n.image}; version ${latest.firmware} is available'
              : 'Version ${n.image}',
        ),
        trailing: newerImage && n.version >= updateVersion
            ? FilledButton(
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => UpdateScreen(app, release: latest),
                  ),
                ),
                child: const Text('Send to the node'),
              )
            : null,
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
    final st = s.status;
    // A guest can clear no key, and makes the node its home by pairing
    // first; between its errands only the last one's reading is known.
    final owned = s.connected && !s.guest;
    final guest = app.guestHere;
    final here = owned ? s.address : guest?.address;
    final atHome = owned ? app.atHome : app.guestAtHome;
    final homeable = owned ? canBeHome(st) : guest?.homeable ?? false;
    final full = guest?.room == 0;
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
        ..._placeHereButton(),
      ],
      if (guest != null && here == null)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text('This node has no place yet, so it cannot be a home.'),
        ),
      if (here != null && !atHome && !homeable)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'A home must be a node that stays on power at a fixed place.',
          ),
        ),
      if (app.homeKeyGone)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'Your home node does not hold your mailbox key: someone paired '
            'with it may have removed it. Make it your home again, or '
            'choose another.',
          ),
        ),
      if (app.holdsOtherKey)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            s.nodeHome?.mine == null
                ? 'This node holds a mailbox key: it is someone\'s home, '
                      'perhaps a former one of yours.'
                : 'This node still holds your mailbox key, from when it was '
                      'your home.',
          ),
        ),
      if (app.othersHome)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text('This node is someone else\'s home.'),
        ),
      if (full)
        const Padding(padding: EdgeInsets.only(top: 8), child: Text(fullNode)),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          if ((owned || guest != null) &&
              here != null &&
              (!atHome || app.homeKeyGone))
            FilledButton(
              onPressed: homeable && !app.connecting && (owned || !full)
                  ? () => _setHome(here, pair: !owned)
                  : null,
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

  /// Makes the node at `a` the home, or none; `pair` for a node the phone
  /// uses as a guest, which it pairs with first.
  Future<void> _setHome(Address? a, {bool pair = false}) async {
    final n = app.contacts.list.length;
    // The card carries the home; the new card goes out only if it
    // changes.
    final changes = a?.toString() != app.me.home?.toString();
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(a == null ? 'No home node' : 'Make this node my home'),
        content: Text(
          (pair
                  ? '$_pairsFirst As your home, it keeps texts to you and '
                        'gives your card a fixed address.\n\n'
                  : '') +
              (!changes
                  ? 'Your card stays as it is, so no card is sent.'
                  : n == 0
                  ? 'Your card changes.'
                  : n == 1
                  ? 'Your card changes, and your new card goes to '
                        'your contact.'
                  : 'Your card changes, and your new card goes to '
                        'your $n contacts, one message each.'),
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
    if (pair) {
      await _connect(() async {
        final r = await app.homeHere();
        if (mounted) say(context, r);
      }, pairing: true);
      return;
    }
    await _try(() async {
      final r = await (a == null ? app.noHome() : app.makeHome(a));
      if (mounted) say(context, r);
    }, 'Setting the home failed');
  }

  /// What pairing with a node the phone uses as a guest takes, and what it
  /// does to the node's keeper.
  static const _pairsFirst =
      'The phone pairs with this node first: you type the six-digit code '
      'its screen shows. A node keeps up to eight phones, and the phones '
      'paired with it before keep theirs.';

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
            ? 'The node has no place yet.'
            : st.mapPoint
            ? 'The node\'s place, set by a person: ${st.address}.'
            : 'The node\'s place, from its GPS: ${st.address}.',
      ),
      const SizedBox(height: 4),
      Text(
        'A node finds its place by GPS. Set it here for a node that cannot '
        'see the sky, such as one indoors: what is set here wins over GPS '
        'until it is cleared.',
        style: Theme.of(context).textTheme.bodySmall,
      ),
      Row(
        children: [
          Expanded(child: _degrees(lat, 'Latitude')),
          const SizedBox(width: 16),
          Expanded(child: _degrees(lon, 'Longitude')),
        ],
      ),
      const SizedBox(height: 8),
      ..._placeHereButton(),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
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

/// Takes the six digits the node's screen shows for a removal.
class _CodeDialog extends StatefulWidget {
  const _CodeDialog();

  @override
  State<_CodeDialog> createState() => _CodeDialogState();
}

class _CodeDialogState extends State<_CodeDialog> {
  final _code = TextEditingController();

  @override
  void dispose() {
    _code.dispose();
    super.dispose();
  }

  bool get _whole => RegExp(r'^\d{6}$').hasMatch(_code.text);

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('The code on the node'),
    content: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'The node\'s screen shows REMOVE and six digits. Type them '
          'within 30 s.',
        ),
        TextField(
          key: const Key('removal code'),
          controller: _code,
          autofocus: true,
          keyboardType: TextInputType.number,
          maxLength: 6,
          decoration: const InputDecoration(labelText: 'Code'),
          onChanged: (_) => setState(() {}),
        ),
      ],
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(
        onPressed: _whole ? () => Navigator.pop(context, _code.text) : null,
        child: const Text('Remove'),
      ),
    ],
  );
}
