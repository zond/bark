// What every screen shares: the person, their contacts, the session with
// the node, the messages sealed and opened, and how the app reaches a
// node over Bluetooth. The model is made of parts, each with one job, in
// `app/`: the links it makes, the search that chooses them, pairing and
// Android's bonds, the home and the card, the person's scan, and a node's
// install.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';
import 'package:universal_ble/universal_ble.dart';

import 'alerts.dart';
import 'ble_link.dart';
import 'contacts.dart';
import 'decision.dart';
import 'link.dart';
import 'me.dart';
import 'messenger.dart';
import 'node_link.dart';
import 'nodes.dart';
import 'phone_notifier.dart';
import 'releases.dart';
import 'session.dart';
import 'store.dart';
import 'updater.dart';

part 'app/connect.dart';
part 'app/home.dart';
part 'app/installs.dart';
part 'app/pairing.dart';
part 'app/scan.dart';
part 'app/search.dart';

/// What the parts of [AppModel] use of each other, each declared in the
/// part that keeps it.
abstract class _AppCore extends ChangeNotifier {
  Session get session;
  Me get me;
  Contacts get contacts;
  Messenger get messenger;
  Updater get updater;
  DateTime Function() get now;

  /// What the phone knows of each node.
  NodeStore get nodes => session.nodes;

  // Bluetooth, as Android allows it ([AppModel]).
  bool get bluetoothAllowed;
  Future<void> checkBluetooth();
  Future<void> Function() get _requestBluetooth;

  // The person's scan ([_Scan]).
  bool get scanning;
  Future<void> stopScan();

  // Installs ([_Installs]).
  Install? get install;
  set install(Install? i);
  bool get _installsNow;
  Awaited? get _awaited;
  void _giveUp(String node);

  // Pairing ([_Pairing]).
  Future<void> Function()? get pairAgain;
  set pairAgain(Future<void> Function()? f);
  bool get _unpairing;
  Future<void> recheckKept();
  Future<bool> _forgetBond(String name, String device);
  Future<void> _forgetBonds(Iterable<FoundNode> heard);
  FoundNode? heardLately(String name);

  // Links ([_Connect]).
  bool get connecting;
  String? get linkDevice;
  LinkNow? get _linkNow;
  Future<void> connect(FoundNode n, {bool again = false});
  Future<void> _connectFound(FoundNode n);
  Future<void> _connectPublic(FoundNode n);

  // The search ([_Search]).
  void _roundsAfresh();
  void _heard(Iterable<FoundNode> heard);
  Future<void> _searchStopped();
  Future<void> stopLooking();
  Situation situation({List<FoundNode>? heard});
  NodeRecord? get guestHere;

  // The home ([_Home]).
  Future<void> _sendQueuedFetch();
}

class AppModel extends _AppCore
    with _Scan, _Installs, _Pairing, _Connect, _Search, _Home {
  final Sodium sodium;
  @override
  final Me me;
  @override
  final Session session;
  @override
  final Contacts contacts;
  @override
  late final Messenger messenger;
  @override
  final DateTime Function() now;
  final Notifier notifier;
  late final Alerts alerts;

  /// The latest release, against this app and the connected node.
  final Releases releases;

  AppModel({
    required this.sodium,
    required this.me,
    required this.session,
    required this.contacts,
    required this.now,
    this.notifier = const NoNotifier(),
    Releases? releases,
    Future<bool> Function()? bluetoothAllowed,
    Future<void> Function()? requestBluetooth,
  }) : releases = releases ?? Releases(MemoryStore('release'), now: now),
       _bluetoothAllowed = bluetoothAllowed ?? UniversalBle.hasPermissions,
       _requestBluetooth = requestBluetooth ?? _askAndroid {
    this.releases.noted = session.note;
    messenger = Messenger(sodium, me, contacts, session, now);
    alerts = Alerts(notifier, contacts);
    session.kept = (r) =>
        alerts.kept(r, session.partsOf(r)).catchError(_unnotified);
    session.addListener(_nodeAddress);
    session.addListener(_installs);
    session.addListener(_queuedFetch);
    session.addListener(_link);
    session.addListener(_search);
    addListener(_link);
    addListener(_search);
    // The countdowns move without a word from the node, and the check for
    // a release is due a day after the last.
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      notifyListeners();
      unawaited(this.releases.check());
    });
  }

  static Future<AppModel> onPhone(Sodium s) async {
    return AppModel(
      sodium: s,
      me: Me(FileStore('me.json'), s, DateTime.now),
      session: Session(store: FileStore('bark.json'), now: DateTime.now)
        // debugPrint, unlike dart:developer's log, reaches the Android log
        // in release builds too, under the tag flutter.
        ..noted = (l) => debugPrint('bark ${logTime(l.at)} ${l.text}'),
      contacts: Contacts(FileStore('contacts.json')),
      now: DateTime.now,
      notifier: await PhoneNotifier.start(),
      releases: Releases(
        FileStore('release.json'),
        now: DateTime.now,
        fetch: fetchLatest,
        app: appVersion,
      ),
    );
  }

  late final Timer _ticker;

  /// The firmware update of the connected node.
  @override
  late final Updater updater = Updater(
    session,
    () => mtu ?? defaultAttMtu,
    release: () => releases.latest,
  );

  /// Everything a screen redraws on.
  Listenable get changes =>
      Listenable.merge([this, session, contacts, me, updater, releases]);

  Future<void> load() async {
    await Future.wait([
      me.load(),
      session.load(),
      contacts.load(),
      releases.load(),
    ]);
    for (final s in [me.store, session.store, contacts.store, releases.store]) {
      if (s.fault != null) session.note(s.fault!);
    }
  }

  /// Keeps the app running while a node is connected by Bluetooth, so
  /// that it reads the inbox with the screen off, and while it searches
  /// for the paired node, so that the texts waiting go out as soon as it
  /// is in reach.
  ///
  /// The app also starts with the phone and looks for its paired nodes,
  /// unless none is still this phone's or the person stopped looking: a
  /// phone that held a link when it was switched off takes it up again
  /// without anyone opening the app.
  void _link() {
    unawaited(alerts.hold(held).catchError(_unnotified));
    final p = firstPaired;
    final boot = p == null || session.stopped
        ? null
        : _looking(shownName(p), bluetooth: true);
    unawaited(alerts.atBoot(boot).catchError(_unnotified));
  }

  /// What the link's notification says: the node connected, or the
  /// search, only once Bluetooth is allowed, so that an app not yet given
  /// it shows nothing. Between rounds, while Bluetooth is off and while
  /// connecting it stays: Android 12 and later refuse to start the service
  /// again from the background.
  Held? get held {
    final on = nodeLine;
    if (on != null) {
      return (
        title: on,
        body: session.connected
            ? 'Texts arrive while the screen is off'
            : 'Checks for texts about once a minute',
      );
    }
    if (!searchOn || !bluetoothAllowed) return null;
    return _looking(sought, bluetooth: bluetoothOn);
  }

  static Held _looking(String node, {required bool bluetooth}) => (
    title: searchLine(node, bluetooth: bluetooth),
    body: bluetooth
        ? 'Texts waiting go out once it is in reach'
        : 'Texts waiting go out once Bluetooth is on',
  );

  /// The line while the phone is on no node and looks for `node`, by
  /// Bluetooth or not.
  static String searchLine(String node, {required bool bluetooth}) => bluetooth
      ? 'Not connected. Looking for $node; texts wait on the phone'
      : 'Not connected. Bluetooth is off: turn it on to reach $node';

  /// The line while the phone is on no node and does not look: the person
  /// stopped the search, or it waits for Bluetooth to be allowed.
  static const notLookingLine = 'Not connected. Texts wait on the phone';

  /// The node the search looks for, as a person reads it: the first of
  /// the paired nodes ([firstPaired]), else a public node.
  String get sought {
    final f = searchFor;
    return f == null ? 'a public node' : shownName(f);
  }

  void _unnotified(Object e) => session.note('Notifying failed: $e');

  /// Asks, once, to notify; `why` shows the person why first and says
  /// whether to go on.
  Future<void> askToNotify(Future<bool> Function() why) async {
    if (me.askedToNotify) return;
    await me.setAskedToNotify();
    if (await notifier.allowed()) return;
    if (await why()) await notifier.ask();
  }

  /// Asks, once, to use Bluetooth, as the app starts: without it there is
  /// no node to send through. `why` shows the person why first and says
  /// whether to go on.
  Future<void> askForBluetooth(Future<bool> Function() why) async {
    if (me.askedBluetooth || !_searchStarted) return;
    await checkBluetooth();
    if (bluetoothAllowed) return;
    await me.setAskedBluetooth();
    if (!await why()) return;
    try {
      await _requestBluetooth();
    } finally {
      await checkBluetooth();
    }
  }

  @override
  final Future<void> Function() _requestBluetooth;
  static Future<void> _askAndroid() async {
    await UniversalBle.requestPermissions();
  }

  /// Whether Android lets the app use Bluetooth, as last checked: a fresh
  /// install has not been allowed yet, and its search would only fail.
  @override
  bool bluetoothAllowed = false;
  final Future<bool> Function() _bluetoothAllowed;

  /// Checks whether Bluetooth is allowed: at start, when the app comes to
  /// the front and when Bluetooth's state changes, as the person may have
  /// allowed it in Android's settings meanwhile.
  @override
  Future<void> checkBluetooth() async {
    bool a;
    try {
      a = await _bluetoothAllowed();
    } catch (_) {
      a = false;
    }
    if (a == bluetoothAllowed) return;
    bluetoothAllowed = a;
    notifyListeners();
  }

  /// The steps to a first text, each done or not, for the first screen's
  /// card until all are done: a name for the card, a node, by the search
  /// or paired, and a contact.
  ({bool name, bool node, bool contact}) get gettingStarted => (
    name: me.name.isNotEmpty,
    node:
        session.connected || nodes.paired.isNotEmpty || me.nodeAddress != null,
    contact: contacts.list.isNotEmpty,
  );

  /// Which node the phone is on, as the first screen, Settings and the
  /// link's notification say it; null while it is on none. A public node
  /// is on between errands too. Why it is on that node is the help's to
  /// say, not the line's.
  String? get nodeLine {
    final n = session.connected ? session.node : null;
    final g = n != null && session.guest ? nodeName(n.id) : guestNode;
    final h = homeName;
    final home = h == null ? '' : '. Home at ${shownName(h)}';
    final installs = _installNote;
    if (g != null && tappedGuest) {
      return 'Connected to ${shownName(g)} as a guest for 30 s$installs$home';
    }
    if (g != null && (n == null || session.guest)) {
      return 'Connected to ${shownName(g)} as a guest$installs$home';
    }
    if (n == null) return null;
    if (atHome) return 'Connected to your home, ${nodeLabel(n.id)}$installs';
    return 'Connected to ${nodeLabel(n.id)}, paired$installs$home';
  }

  @override
  void dispose() {
    _ticker.cancel();
    _scanTimer?.cancel();
    _availability?.cancel();
    _searchStarted = false;
    if (!_wake.isCompleted) _wake.complete();
    session.removeListener(_nodeAddress);
    session.removeListener(_installs);
    session.removeListener(_queuedFetch);
    session.removeListener(_link);
    session.removeListener(_search);
    updater.dispose();
    releases.dispose();
    session.dispose();
    contacts.dispose();
    me.dispose();
    super.dispose();
  }
}
