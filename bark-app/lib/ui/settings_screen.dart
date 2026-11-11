// Settings, in the order a person looks for things: the node the phone
// is on and the nodes it is paired with; the home node; on a paired node,
// its own settings, whether it serves phones without pairing, its place
// and the phones paired with it; updates of the app and the node; the
// person's name and backup words; the battery exemption that keeps the
// link with the screen off; then Debug. The Node, Home node and Updates
// sections are widgets of their own.

import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';
import 'package:geolocator_platform_interface/geolocator_platform_interface.dart'
    show GeolocatorPlatform, LocationPermission;

import '../app.dart';
import '../app_layer.dart';
import '../link.dart';
import '../node_link.dart';
import 'common.dart';
import 'debug_screen.dart';
import 'home_section.dart';
import 'node_section.dart';
import 'settings_parts.dart';
import 'updates_section.dart';

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

  /// A connection the person asks for here, in whichever section.
  late final _connect = ConnectAsk(app);

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
              BondKeptLine(app),
              NodeSection(app, _connect),
              HomeSection(app, _connect),
              ..._thisNode(),
              UpdatesSection(app),
              SettingsHeading('My name'),
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
              ..._battery(),
              const SizedBox(height: 24),
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.bug_report_outlined),
                title: const Text('Debug'),
                subtitle: const Text('What the link says, for a bug report'),
                onTap: () => Navigator.of(context)
                    .push(MaterialPageRoute(builder: (_) => DebugScreen(app))),
              ),
            ],
          ),
        ),
      ),
    ),
  );

  /// On a paired node, its own settings: whether it serves phones without
  /// pairing, its place and the phones paired with it.
  List<Widget> _thisNode() {
    final s = app.session;
    if (!s.connected || s.guest) return [];
    return [
      SettingsHeading('This node'),
      ..._public(),
      ..._place(),
      ..._phones(),
    ];
  }

  /// The switch of whether the node serves phones that have not paired
  /// with it.
  List<Widget> _public() {
    final p = app.session.nodePublic;
    if (p == null) return [];
    return [
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Public'),
        subtitle: const Text('Any phone near it can text through it'),
        value: p,
        onChanged: (v) => trySay(
          context,
          () => app.session.writePublic(v),
          'Setting Public failed',
        ),
      ),
    ];
  }

  /// The node's phones: those paired with it, and those whose home it
  /// is, a record holding both in both lists (spec "Phone link", Phones).
  /// Another's row has Remove, the person's own paired row Unpair; their
  /// own home is changed under Home node.
  List<Widget> _phones() {
    final s = app.session;
    final rs = s.phones;
    if (rs == null) return [];
    final paired = rs.where((r) => r.paired).toList();
    final homes = rs.where((r) => r.home).toList();
    return [
      _subheading('Phones paired with this node', help: _phonesHelp),
      if ((s.node?.room ?? 1) == 0)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            'Full: remove one before another can pair.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
      for (final r in paired) _phoneRow(r, home: false),
      if (homes.isNotEmpty) ...[
        _subheading('Phones with a home here'),
        for (final r in homes) _phoneRow(r, home: true),
      ],
    ];
  }

  Widget _subheading(String t, {String? help}) => Padding(
    padding: EdgeInsets.only(top: 16, bottom: help == null ? 4 : 0),
    child: Row(
      children: [
        Flexible(child: Text(t, style: Theme.of(context).textTheme.titleSmall)),
        if (help != null) HelpButton(t, help),
      ],
    ),
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
      if (r.paired) '$whoCap can no longer use the node unless paired again.',
      if (r.home) 'The node stops keeping their mail.',
    ];
    final (ok, gone) = await _whileListed<bool>(
      r,
      (context) => AlertDialog(
        title: Text(
          r.paired && r.home
              ? 'Remove $who and its home?'
              : r.paired
              ? 'Remove $who?'
              : 'Remove this home?',
        ),
        content: Text(
          '${parts.join(' ')} You type the code the node then shows.',
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
    if (!mounted) return;
    if (gone) {
      say(context, 'Already gone.');
      return;
    }
    if (ok != true) return;
    await _removeByCode(r);
  }

  /// Whether the node's list, as last read, still holds `r` with what its
  /// removal takes; with no list, as once the link ended, it is not known
  /// to have gone.
  bool _listed(PhoneRecord r) =>
      app.session.phones?.any(
        (x) =>
            x.number == r.number &&
            (x.paired || !r.paired) &&
            (x.home || !r.home),
      ) ??
      true;

  /// Shows a dialog of `r`'s removal, which closes by itself once the list,
  /// read again as it changes, no longer holds `r`: another phone removed
  /// it first. Its answer, and whether `r` went.
  Future<(T?, bool)> _whileListed<T>(
    PhoneRecord r,
    WidgetBuilder builder,
  ) async {
    final s = app.session;
    BuildContext? shown;
    var gone = false;
    void check() {
      if (gone || _listed(r)) return;
      gone = true;
      final c = shown;
      if (c != null && c.mounted) Navigator.pop(c);
    }

    s.addListener(check);
    try {
      final v = await showDialog<T>(
        context: context,
        builder: (c) {
          shown = c;
          return builder(c);
        },
      );
      return (v, gone);
    } finally {
      s.removeListener(check);
    }
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
      final (code, gone) = await _whileListed<String>(
        r,
        (context) => const _CodeDialog(),
      );
      if (!mounted) return;
      if (gone) {
        say(context, 'Already gone.');
        return;
      }
      if (code == null) return;
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
          say(context, 'Already gone.');
          return;
        }
        final again = await confirm(
          context,
          'Not removed',
          'Wrong code, or too late. Each code works once.',
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
    final others = app.nodes.paired.length > 1
        ? ' It stays paired with your other nodes.'
        : '';
    // With the last phone gone a node is public, whatever its owners set.
    final last = !(s.phones ?? const []).any((r) => r.paired && !r.mine);
    final after = last ? ' With no phone left, it turns public.' : '';
    final done = last ? 'Unpaired. The node is public.' : 'Unpaired.';
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Unpair ${shownName(name)}?'),
        content: Text(
          'The node and this phone forget each other; texts and contacts '
          'stay.$after$others',
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
    if (ok != true || !mounted) return;
    await trySay(context, () async {
      // The home is the node's mailbox key, which it keeps.
      final home = app.atHome ? ' It stays your home.' : '';
      final forgotten = await app.unpair();
      if (!mounted) return;
      if (forgotten) {
        say(context, '$done$home');
        return;
      }
      await askToForget(
        context,
        app,
        '$done$home Forget the node in Bluetooth settings too: the phone '
        'kept its pairing.',
      );
    }, 'Unpairing failed');
  }

  List<Widget> _battery() {
    final e = _exempt;
    if (e == null) return [];
    return [
      SettingsHeading('Battery'),
      Text(
        e
            ? 'bark may run with the screen off.'
            : 'With the screen off, Android may close bark\'s link after '
                  'some hours.',
      ),
      if (!e) ...[
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(
            onPressed: app.notifier.askBatteryExempt,
            child: const Text('Let bark run'),
          ),
        ),
      ],
    ];
  }

  /// What the node's phones are, for whoever asks.
  static const _phonesHelp =
      'A node keeps up to eight phones. Anyone at it can pair, by the code '
      'on its screen, and every paired phone can change its settings, so '
      'remove a phone you do not know or one that is gone. A removal also '
      'takes the code on the node\'s screen, to show you are there.\n\n'
      'A phone with a home here has its mail kept at this node.';

  List<Widget> _backup() => [
    SettingsHeading('Backup'),
    Text(
      app.me.backedUp
          ? 'Your 12 words bring your identity back on another phone.'
          : 'Write down your 12 words: a lost phone without them loses your '
                'identity.',
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
      'This phone takes the identity of the words. Contacts reach it only '
          'once they have its card.',
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

  /// The node's place, from its GPS or set here.
  List<Widget> _place() {
    final s = app.session;
    final st = s.status;
    return [
      _subheading('Place'),
      Text(
        st == null || !st.placed
            ? 'The node has no place yet.'
            : st.mapPoint
            ? 'Set by a person: ${st.address}.'
            : 'From its GPS: ${st.address}.',
      ),
      const SizedBox(height: 4),
      Text(
        'For a node indoors, out of GPS reach. It wins over GPS until '
        'cleared.',
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
          'Type the six digits under REMOVE on the node\'s screen, within '
          '30 s.',
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
