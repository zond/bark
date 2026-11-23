// Settings, in the order a person looks for things: the node the phone
// is on and the nodes it is paired with; the home node; on a paired node,
// its own settings, whether it serves phones without pairing, its place
// and the phones paired with it; updates of the app and the node; the
// person's name, backup words and identity file; the battery exemption
// that keeps the link with the screen off; then Debug. The Node, Home node, Updates and
// My identity sections are widgets of their own.

import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';

import '../app.dart';
import '../app_layer.dart';
import '../l10n.dart';
import '../link.dart';
import '../phone_place.dart';
import 'common.dart';
import 'debug_screen.dart';
import 'home_section.dart';
import 'identity_section.dart';
import 'node_section.dart';
import 'people.dart';
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
      tr.settingsBackupAnnounce,
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
    if (app.links.scanning) app.links.stopScan();
    super.dispose();
  }

  /// The phone's position, after asking for location if needed; null,
  /// with the person told why, without one.
  Future<Position?> _phonePosition() async {
    try {
      return await app.place.here();
    } on NoPlace catch (e) {
      if (mounted) say(context, said(e));
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
        label: Text(tr.settingsPlaceHere),
        onPressed: _placeHere,
      ),
    ),
    if (_placeSet != null)
      Text(
        tr.settingsPlaceSet(_placeSet!),
        style: Theme.of(context).textTheme.bodySmall,
      ),
  ];

  Future<void> _setMapPoint() async {
    final la = double.tryParse(lat.text.trim());
    final lo = double.tryParse(lon.text.trim());
    if (la == null || lo == null || la.abs() > 90 || lo.abs() > 180) {
      say(context, tr.settingsDegreesPlease);
      return;
    }
    await app.session.setMapPoint(Position.fromDegrees(la, lo));
  }

  @override
  Widget build(BuildContext context) => Watch(
    app,
    (context) => Scaffold(
      appBar: AppBar(title: Text(tr.settings)),
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
              SettingsHeading(tr.commonMyName),
              ListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(app.me.name.isEmpty ? tr.nameNotSet : app.me.name),
                subtitle: Text(tr.settingsNameHow),
                trailing: IconButton(
                  tooltip: tr.changeName,
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
              IdentitySection(app),
              ..._battery(),
              const SizedBox(height: 24),
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.bug_report_outlined),
                title: Text(tr.debug),
                subtitle: Text(tr.settingsDebugHow),
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
      SettingsHeading(tr.settingsThisNode),
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
        title: Text(tr.settingsPublic),
        subtitle: Text(tr.settingsPublicHow),
        value: p,
        onChanged: (v) => trySay(
          context,
          () => app.session.writePublic(v),
          tr.settingsPublicFailed,
        ),
      ),
    ];
  }

  /// The node's phones: those paired with it, and those whose home it
  /// is, a record holding both in both lists (spec "Phone link", Phones).
  /// Another's row has Remove, a home's Make owner too; the person's own
  /// paired row Unpair, and their own home is changed under Home node.
  /// With no owner, a paired phone may take the seat at its home.
  List<Widget> _phones() {
    final s = app.session;
    final rs = s.phones;
    if (rs == null) return [];
    final paired = rs.where((r) => r.paired).toList();
    final homes = rs.where((r) => r.home).toList();
    Widget row(PhoneRecord r, {required bool home}) => RecordRow(
      app,
      r,
      home: home,
      atNode: true,
      ownAction: TextButton(
        onPressed: _askUnpair,
        child: Text(tr.settingsUnpair),
      ),
    );
    return [
      _subheading(tr.settingsPhonesPaired, help: tr.settingsPhonesHelp),
      if ((s.node?.room ?? 1) == 0)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            tr.settingsPhonesFull,
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
      for (final r in paired) row(r, home: false),
      if (homes.isNotEmpty) ...[
        _subheading(tr.settingsPhonesHome),
        for (final r in homes) row(r, home: true),
        if (!homes.any((r) => r.owner))
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: Text(tr.settingsNoOwner),
            trailing: app.mailbox.atHome
                ? TextButton(
                    onPressed: () => trySay(context, () async {
                      final r = await app.mailbox.becomeOwner();
                      if (mounted) say(context, r);
                    }, tr.failed),
                    child: Text(tr.settingsBecomeOwner),
                  )
                : null,
          ),
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

  Future<void> _askUnpair() async {
    final s = app.session;
    final name = nodeName(s.node!.id);
    final others = app.session.nodes.paired.length > 1;
    // With the last phone gone a node is public, whatever its phones set.
    final last = !(s.phones ?? const []).any((r) => r.paired && !r.mine);
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(tr.settingsUnpairTitle(shownName(name))),
        content: Text(
          [
            tr.settingsUnpairHow,
            if (last) tr.settingsUnpairLast,
            if (others) tr.settingsUnpairOthers,
          ].join(' '),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(tr.settingsUnpair),
          ),
        ],
      ),
    );
    if (ok != true || !mounted) return;
    await trySay(context, () async {
      // The home is the node's mailbox key, which it keeps.
      final done = [
        last ? tr.settingsUnpairedPublic : tr.settingsUnpaired,
        if (app.mailbox.atHome) tr.staysYourHome,
      ];
      final forgotten = await app.links.unpair();
      if (!mounted) return;
      if (forgotten) {
        say(context, done.join(' '));
        return;
      }
      await askToForget(
        context,
        app,
        [...done, tr.forgetInBluetoothToo].join(' '),
      );
    }, tr.settingsUnpairFailed);
  }

  List<Widget> _battery() {
    final e = _exempt;
    if (e == null) return [];
    return [
      SettingsHeading(tr.settingsBattery),
      Text(e ? tr.settingsBatteryExempt : tr.settingsBatteryNotExempt),
      if (!e) ...[
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(
            onPressed: app.notifier.askBatteryExempt,
            child: Text(tr.settingsLetRun),
          ),
        ),
      ],
    ];
  }

  List<Widget> _backup() => [
    SettingsHeading(tr.settingsBackup),
    Text(app.me.backedUp ? tr.settingsBackedUp : tr.settingsNotBackedUp),
    const SizedBox(height: 8),
    Wrap(
      spacing: 8,
      children: [
        FilledButton(
          onPressed: () => _showWords(),
          child: Text(tr.settingsShowWords),
        ),
        OutlinedButton(onPressed: _restore, child: Text(tr.settingsRestore)),
      ],
    ),
  ];

  Future<void> _showWords() async {
    await showDialog<void>(
      context: context,
      builder: (context) => Secret(
        child: AlertDialog(
          title: Text(tr.settingsMyWords),
          content: SelectableText(
            app.me.phrase.join(' '),
            style: const TextStyle(fontSize: 18),
          ),
          actions: [
            FilledButton(
              onPressed: () => Navigator.pop(context),
              child: Text(tr.settingsWroteThem),
            ),
          ],
        ),
      ),
    );
    await app.me.setBackedUp();
  }

  Future<void> _restore() async {
    final go = await confirm(
      context,
      tr.settingsRestoreTitle,
      tr.settingsRestoreHow,
      tr.settingsRestoreButton,
    );
    if (!go || !mounted) return;
    final w = await askText(context, tr.settingsYourWords, '', secret: true);
    if (w == null || w.isEmpty) return;
    final seed = seedOfPhrase(w);
    if (seed == null) {
      if (mounted) say(context, tr.settingsNotWords);
      return;
    }
    await app.moving.restoreWords(seed);
    if (mounted) say(context, tr.settingsRestored);
  }

  /// The node's place, from its GPS or set here.
  List<Widget> _place() {
    final s = app.session;
    final st = s.status;
    return [
      _subheading(tr.settingsPlace),
      Text(
        st == null || !st.placed
            ? tr.settingsNoPlace
            : st.mapPoint
            ? tr.settingsPlaceByPerson('${st.address}')
            : tr.settingsPlaceByGps('${st.address}'),
      ),
      const SizedBox(height: 4),
      Text(tr.settingsPlaceHow, style: Theme.of(context).textTheme.bodySmall),
      Row(
        children: [
          Expanded(child: _degrees(lat, tr.settingsLatitude)),
          const SizedBox(width: 16),
          Expanded(child: _degrees(lon, tr.settingsLongitude)),
        ],
      ),
      const SizedBox(height: 8),
      ..._placeHereButton(),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          FilledButton(
            onPressed: _setMapPoint,
            child: Text(tr.settingsPlaceSetButton),
          ),
          OutlinedButton(
            onPressed: () => s.setMapPoint(null),
            child: Text(tr.settingsPlaceClear),
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
