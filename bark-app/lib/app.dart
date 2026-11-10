// What every screen shares: the person, their contacts, the session with
// the node, the messages sealed and opened, and how the app reaches a
// node, by Bluetooth or the fake.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';
import 'package:universal_ble/universal_ble.dart';

import 'alerts.dart';
import 'ble_link.dart';
import 'contacts.dart';
import 'fake_node.dart';
import 'link.dart';
import 'me.dart';
import 'messenger.dart';
import 'node_link.dart';
import 'phone_notifier.dart';
import 'releases.dart';
import 'session.dart';
import 'store.dart';
import 'updater.dart';

/// The steps of a connection before its link is attached.
enum Linking {
  none,

  /// Connecting, and asking for the MTU.
  connecting,

  /// Android asks the person for the code on the node's screen.
  pairing,

  /// The bond is made; the app goes on to connect.
  paired,

  /// Connected; the app reads the node.
  attaching,
}

/// The node heard strongest of `nodes`, the likeliest to hold a link.
FoundNode? _strongest(Iterable<FoundNode> nodes) {
  FoundNode? best;
  for (final n in nodes) {
    if ((n.rssi ?? noRssi) > (best?.rssi ?? noRssi) || best == null) {
      best = n;
    }
  }
  return best;
}

/// The node a round of the search connects to, of those it `heard`, in
/// this order: the `home` when it is one of the `paired` nodes, as it
/// serves the person whole and keeps their mail; else the paired node
/// heard strongest; else the public node heard strongest, never one of
/// `lost` nor of `stale`, whose bond the phone still holds and which ends
/// a link encrypted with it (spec "Phone link", The replaced phone,
/// Public); else none.
///
/// The order holds among the nodes heard at a signal a link holds at
/// ([FoundNode.usable]): a node heard weak is passed over for one further
/// down heard better, as a link to it would not hold. With none usable,
/// the node heard strongest, of whatever kind, as signal alone then says
/// which link is likeliest to hold (roadmap D171).
///
/// While the phone has a link to the node `on`, the choice is only a node
/// higher in the order than it, heard usable: the home over another paired
/// node, a paired node over a public one. Never one beside it, as two of
/// one place in the order heard by turns at like signals would move the
/// phone back and forth; and never a weak one over the link it has
/// (roadmap D172).
FoundNode? chooseNode(
  Iterable<FoundNode> heard, {
  String? home,
  Set<String> paired = const {},
  Set<String> lost = const {},
  Set<String> stale = const {},
  String? on,
}) {
  int place(String name) => name == home && paired.contains(name)
      ? 0
      : paired.contains(name)
      ? 1
      : 2;
  // A node the phone may use at all, whatever its signal.
  bool may(FoundNode n) =>
      paired.contains(n.name) ||
      (n.public && !lost.contains(n.name) && !stale.contains(n.name));
  final good = heard.where((n) => n.usable && may(n));
  final own = good.where((n) => paired.contains(n.name));
  final FoundNode? n;
  if (good.isEmpty) {
    n = on == null ? _strongest(heard.where(may)) : null;
  } else if (own.isEmpty) {
    n = _strongest(good);
  } else {
    n = own.where((n) => place(n.name) == 0).firstOrNull ?? _strongest(own);
  }
  if (n == null || on == null) return n;
  return place(n.name) < place(on) ? n : null;
}

/// How often the app reads a public node's ring for the person's mail
/// while it has nothing to send, short of the paired node: a minute, the
/// span the spec takes for a person on foot to stay within a relay's reach
/// ("Where a text goes", Why a minute), which a sender's app takes as how
/// long a reply should go to where the person wrote from; a reply that
/// came there is thus read within about as long as it was aimed there.
/// Four of the search's rounds of 15 s, so it costs the phone no scan
/// more within Android's five in 30 s. The node pays for it in its one
/// guest link: an errand of a few seconds, say 5, a minute takes a
/// twelfth of the link, so a few phones in reach seldom meet there, and
/// one that does is closed and tries next round. Mail waits in the ring
/// meanwhile, so a longer interval delays it and loses nothing.
const Duration guestCheck = Duration(minutes: 1);

/// How often the phone, on a paired node that is not the home, scans for
/// the home, and moves to it once heard (roadmap D172): [guestCheck]'s
/// minute, the span the spec takes for a person on foot to stay within a
/// relay's reach, so a home that comes into reach is taken within about
/// as long as the person stays near it. Not every round of [lookRound]: on
/// a standing link a scan serves only this, and a scan of [lookScan] a
/// minute listens some 7 % of the time, against 27 % every 15 s, for a
/// home reached at most 45 s sooner. It takes at most one of Android's
/// five scans in 30 s.
const Duration lookAround = guestCheck;

/// The search's rounds in a row that must reach none of the paired nodes
/// before the app calls them out of reach and goes on errands at a public
/// node: two, 30 s at a round every [lookRound]. A round's scan of four
/// advertising intervals hears a node in reach with the screen on, but
/// with the screen off Android listens less and one round may miss it
/// ([lookScan]), so one miss is no proof. A round that hears one moves the
/// phone back at once, as a paired node serves the person whole. The two
/// differ on purpose: at the edge of a paired node's reach, heard in some
/// rounds and not others, the phone stays with it rather than going back
/// and forth, and its texts and the replies to them do not go by a public
/// node it then leaves (spec "Phone link", Which node a phone uses;
/// roadmap D163, D166).
const int ownGone = 2;

/// How long the line says a node is updated, once it is back on the new
/// version: a minute, long enough to be seen when the person looks.
const Duration updatedShown = Duration(minutes: 1);

/// A node's install of an image it gathered over the mesh, as the app
/// follows it (spec "Updates over the mesh", the install; roadmap D182):
/// the node's name, the version, when the link ended for the reboot and
/// when the node came back on that version.
typedef Install = ({String node, int version, DateTime? gone, DateTime? back});

/// The firmware version whose low 16 bits are `low`, as Status tells it,
/// of a node that runs `image`: the first above it.
int installVersion(int image, int low) {
  final v = image - (image & 0xffff) + low;
  return v > image ? v : v + 0x10000;
}

/// What a person reads of a node that keeps as many phones as it can and
/// so takes no more pairings (spec "Phone link", Pairing).
const String fullNode = 'This node is full: eight phones are paired with it.';

/// Whether a node, by its status, can be a home: one that stays on power
/// at a fixed place, so that its address stays (spec "Mailbox").
bool canBeHome(Status? s) => s != null && !s.sleepy && !s.carried;

/// A public node as the phone's last errand there read it: how to reach
/// it, its address once it has a place, and whether it can be a home
/// ([canBeHome]), and how many more phones may pair with it, `room`, 0 on
/// a node that would refuse a pairing. Between errands the link is down
/// and the session knows none of it.
typedef GuestNode = ({
  FoundNode found,
  Address? address,
  bool homeable,
  int? room,
});

class AppModel extends ChangeNotifier {
  final Sodium sodium;
  final Me me;
  final Session session;
  final Contacts contacts;
  late final Messenger messenger;
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

  /// Whether to connect to the fake node instead of Bluetooth.
  bool fake = false;
  FakeNode? fakeNode;
  final Map<String, FoundNode> found = {};
  bool scanning = false;

  /// The last scan found nothing: it was stopped with none found, or has
  /// run [seenWait], ten of a node's advertising intervals, without one.
  bool get noneFound => found.isEmpty && (_scanEnded || _scanLong);
  bool _scanEnded = false;
  bool _scanLong = false;
  Timer? _scanTimer;
  int? mtu;

  /// Where a connection stands until the link is attached.
  Linking linking = Linking.none;

  /// A connection is being made: no other may start, and the search
  /// does not scan. A tap's covers the whole of its connect, from the
  /// close of the old link to the new link up, as the search would take
  /// another node in the moments between ([_byTap]).
  bool get connecting => linking != Linking.none || _tapHeld;

  /// The device of the link connected or being connected: a found
  /// node's ID, or [fakeDevice].
  String? linkDevice;
  static const fakeDevice = 'fake';

  /// After a connect found the node no longer this phone's: forgets the
  /// old bond and connects again, which pairs.
  Future<void> Function()? pairAgain;

  /// The nodes, by name, whose links failed for a bond the phone holds and
  /// the node does not, each with its device ([StaleBond]). The search
  /// passes them over until Android no longer lists the bond, as every
  /// link to them would fail the same way.
  final Map<String, String> staleBonds = {};

  /// A node of [staleBonds], for the line that tells the person.
  String? get staleBond => staleBonds.keys.firstOrNull;

  /// Drops from [staleBonds] each node whose bond Android no longer lists:
  /// the person has forgotten it in the phone's settings.
  Future<void> recheckStale() async {
    for (final MapEntry(key: name, value: device) in [...staleBonds.entries]) {
      if (await BleLink.bonded(device)) continue;
      staleBonds.remove(name);
      session.note('Pairing: the phone no longer holds a bond for $name');
      notifyListeners();
    }
  }

  /// The owner gives the connected node up: the node forgets this phone's
  /// bond, then the phone forgets its own, as Android would encrypt every
  /// later link to the node with it, a guest's too, and the node refuses
  /// it (spec "Phone link", Public). Returns whether the phone's bond is
  /// gone; where Android keeps it, the person forgets the node in the
  /// phone's Bluetooth settings, and [staleBond] says so meanwhile.
  Future<bool> unpair() async {
    _unpairing = true;
    notifyListeners();
    try {
      return await _unpair();
    } finally {
      _unpairing = false;
      notifyListeners();
    }
  }

  /// An unpairing is under way: the search waits, as the node ends the
  /// link while the phone still holds the bond, and a connect with it
  /// then would find the node no longer this phone's.
  bool _unpairing = false;

  Future<bool> _unpair() async {
    final device = linkDevice;
    final n = session.node;
    await session.unpair();
    if (device == null || device == fakeDevice || n == null) return true;
    final name = nodeName(n.id);
    // Nothing learned of the node unpaired bears on the next.
    _ownMissed = 0;
    _guestAt = null;
    final gone = await BleLink.forget(device);
    if (gone) {
      session.note('Pairing: the phone forgot its bond with $name too');
    } else {
      session.note('Pairing: the phone kept its bond with $name');
      staleBonds[name] = device;
      notifyListeners();
    }
    return gone;
  }

  /// The device of each node heard in this run, by name: Android names a
  /// bond by the device, which the app learns only by hearing the node.
  final Map<String, String> _devices = {};

  void _heard(Iterable<FoundNode> nodes) {
    for (final n in nodes) {
      if (n.name == n.deviceId) continue;
      _devices[n.name] = n.deviceId;
      _lastHeard[n.name] = (found: n, at: now());
    }
  }

  /// Each node heard in this run, by name, as last heard and when.
  final Map<String, ({FoundNode found, DateTime at})> _lastHeard = {};

  /// The person taps a node they are paired with in Settings: the app
  /// connects to it as its owner, as a tap on it in a scan does. A node
  /// heard within the span the search takes to call a node out of reach
  /// ([ownGone] rounds) is taken as heard; another is listened for first,
  /// one round's scan, and [NotSeen] goes on if it is not heard.
  Future<void> connectPaired(String name) async {
    if (_tapHeld) return;
    final f = fakeNode;
    if (fake && f != null && name == nodeName(f.nodeId)) return connectFake();
    await _byTap(() async {
      var n = heardLately(name);
      if (n == null) {
        await stopScan();
        final heard = await listen(until: (f) => f.name == name);
        _heard(heard.values);
        n = heard.values.where((f) => f.name == name).firstOrNull;
      }
      if (n == null) throw NotSeen(shownName(name));
      if (connectedTo(n.deviceId) && !session.guest) return;
      await _connect(n);
    });
  }

  /// The node `name` as last heard, if heard within the span the search
  /// takes to call a node out of reach ([ownGone] rounds): in reach.
  FoundNode? heardLately(String name) {
    final last = _lastHeard[name];
    return last != null && now().difference(last.at) < lookRound * ownGone
        ? last.found
        : null;
  }

  /// Whether `name`, a paired node no longer this phone's, may be paired
  /// with again now ([pairLost]): heard lately, or the fake node.
  bool lostInReach(String name) {
    final f = fakeNode;
    if (fake && f != null && name == nodeName(f.nodeId)) return true;
    return heardLately(name) != null;
  }

  /// The person pairs again with `name`, a paired node that no longer
  /// knows this phone, by the code on its screen, as **Pair** on its row
  /// in a scan does ([connect]).
  Future<void> pairLost(String name) async {
    final f = fakeNode;
    if (fake && f != null && name == nodeName(f.nodeId)) {
      f.pair();
      return connectFake();
    }
    final n = heardLately(name);
    if (n == null) throw NotSeen(shownName(name));
    await connect(n);
  }

  /// Forgets `name`, a paired node no longer this phone's: the app counts
  /// it paired no more, and has Android remove its bond with the node, as
  /// [unpair] does, where it has heard the node in this run (roadmap
  /// D173). Returns true once no bond is left, false where Android kept
  /// it ([staleBond] then says so), and null where the app has not heard
  /// the node: it removes the bond once it does ([_bondedNode]), or the
  /// person forgets the node in the phone's Bluetooth settings. The home
  /// stays, as the node keeps the mailbox key.
  Future<bool?> forget(String name) async {
    await session.forget(name);
    final device = _devices[name];
    if (device == null) return null;
    if (!await BleLink.bonded(device)) return true;
    if (await BleLink.forget(device)) {
      session.note('Pairing: the phone forgot its bond with $name too');
      return true;
    }
    session.note('Pairing: the phone kept its bond with $name');
    staleBonds[name] = device;
    notifyListeners();
    return false;
  }

  /// Keeps the node's address for the card, and the home's.
  void _nodeAddress() {
    final a = session.address;
    if (a == null || !session.connected) {
      // The link ended: the card says when it was last up.
      if (!session.connected && me.linked) unawaited(me.unlinked());
      return;
    }
    if (!me.linked || a.toString() != me.nodeAddress?.toString()) {
      unawaited(me.setNodeAddress(a));
    }
    // The home is the node, not the spot: a fixed, awake node's address is
    // its stored place, which moves only at boot or when it turns fixed,
    // and the card and texts to the old one would miss it. A carried
    // node's address is its live fix, which moves every second, and would
    // change the card as often; a sleepy node's is its anchor's, which
    // changes as it picks anchors. The changed card waits for the person to
    // send it.
    final s = session.status;
    if (s == null || s.carried || s.sleepy) return;
    if (atHome && a.toString() != me.home.toString()) unawaited(me.setHome(a));
  }

  /// The install the app follows, the last a node told of.
  Install? install;

  void _installs() {
    final n = session.connected ? session.node : null;
    final i = install;
    if (n == null) {
      if (i != null && i.gone == null && i.back == null) {
        install = (node: i.node, version: i.version, gone: now(), back: null);
        session.note('${i.node} reboots to install version ${i.version}');
      }
      return;
    }
    final status = session.status;
    if (status == null) return;
    final name = nodeName(n.id);
    final low = status.installing;
    if (low != 0) {
      final version = installVersion(n.image, low);
      if (i?.node != name || i?.version != version || i?.gone != null) {
        install = (node: name, version: version, gone: null, back: null);
        session.note('$name drains to install version $version');
      }
    } else if (i?.node == name && i!.back == null) {
      final on = n.image == i.version && i.gone != null;
      install = on
          ? (node: name, version: i.version, gone: i.gone, back: now())
          : null;
      session.note(
        on
            ? '$name runs version ${i.version}, installed over the mesh'
            : '$name does not install version ${i.version} now',
      );
    }
  }

  /// The node in use tells it drains to install the version [install]
  /// names.
  bool get _installsNow {
    final i = install;
    final n = session.connected ? session.node : null;
    return i != null && i.gone == null && n != null && nodeName(n.id) == i.node;
  }

  /// A paired node rebooted to install a version and is not yet back
  /// within [swapAway]: the search's rounds do not count it out of reach.
  bool get _installWaits {
    final i = install;
    final gone = i?.gone;
    return gone != null &&
        i!.back == null &&
        pairedNodes.contains(i.node) &&
        now().difference(gone) < swapAway;
  }

  /// The line while a paired node is away to install a version; null
  /// otherwise.
  String? get installingLine {
    if (!_installWaits || session.connected) return null;
    final i = install!;
    return '${shownName(i.node)} is installing version ${i.version}, back in '
        'about a minute';
  }

  /// Why the node is or was away for an install, for the help; null when
  /// none is under way.
  String? get installHelp {
    if (!_installsNow && installingLine == null) return null;
    final i = install!;
    return '${shownName(i.node)} got version ${i.version} from the nodes '
        'around it. It passes on the texts it holds, at most ten minutes, '
        'then restarts for about a minute. The phone reconnects by itself; '
        'nothing is lost.';
  }

  /// What the line adds of the connected node's install: that it is
  /// installing a version, or that it came back on it within
  /// [updatedShown].
  String get _installNote {
    final i = install;
    if (i == null || !session.connected) return '';
    if (_installsNow) return ', installing version ${i.version}';
    final back = i.back;
    final n = session.node;
    if (back != null &&
        n != null &&
        nodeName(n.id) == i.node &&
        now().difference(back) < updatedShown) {
      return ', updated to version ${i.version}';
    }
    return '';
  }

  /// The card changed and has not gone to the contacts. Without contacts
  /// there is no one to send it to.
  bool get cardUnsent => me.cardUnsent && contacts.list.isNotEmpty;

  /// Why the card changed, for the line that offers to send it.
  String get cardChange => me.home == null
      ? 'Your card changed: no home'
      : 'Your card changed: new home address';

  /// Keeps the app running while a node is connected by Bluetooth, so
  /// that it reads the inbox with the screen off, and while it searches
  /// for the paired node, so that the texts waiting go out as soon as it
  /// is in reach. The fake node runs in the app and is not a connected
  /// device.
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
    final on = fake ? null : nodeLine;
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
    if (me.askedBluetooth || fake || !_searchStarted) return;
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

  final Future<void> Function() _requestBluetooth;
  static Future<void> _askAndroid() async {
    await UniversalBle.requestPermissions();
  }

  /// The steps to a first text, each done or not, for the first screen's
  /// card until all are done: a name for the card, a node, by the search
  /// or paired, and a contact.
  ({bool name, bool node, bool contact}) get gettingStarted => (
    name: me.name.isNotEmpty,
    node:
        session.connected ||
        session.pairedWith.isNotEmpty ||
        me.nodeAddress != null,
    contact: contacts.list.isNotEmpty,
  );

  /// The connected node is the person's home.
  bool get atHome {
    final n = session.node;
    return n != null && messenger.isHome(n.id);
  }

  /// The connected node holds a mailbox key the phone can clear and is
  /// not the person's home: a former home of theirs.
  bool get holdsOtherKey {
    final h = session.nodeHome;
    return !atHome && h != null && h.mine;
  }

  /// The connected node, not the person's home, is someone else's. Only
  /// its keeper, or a removal by the code on its screen, takes that key.
  bool get othersHome {
    final h = session.nodeHome;
    return !atHome && h != null && !h.mine && h.set;
  }

  /// The person's home does not hold their mailbox key now: a phone
  /// paired with it may have removed it ([Session.homeRemoved]).
  bool get homeKeyGone =>
      atHome && session.nodeHome?.mine == false && session.homeRemoved;

  /// Fetches to the node's address; returns what to tell the person. Only
  /// the person's word fetches, as fetched mail crosses the city twice
  /// (roadmap D128).
  Future<String> fetchNow() async {
    if (me.home == null) return 'No home node to fetch from';
    if (!session.connected) {
      await me.setFetchQueued(true);
      return 'The fetch waits for a node';
    }
    if (atHome) {
      return 'At your home, your mail comes by itself';
    }
    if (session.fetchWaiting != null) {
      return 'Still waiting for the last fetch';
    }
    final here = session.address;
    if (here == null) {
      return 'This node has no place yet, so mail cannot come back to it';
    }
    session.fetchNews = null;
    await messenger.fetch(here);
    return 'Fetching your mail';
  }

  /// Sends the queued fetch once a node is linked and has an address,
  /// from it; at the home there is nothing to fetch, as the mail is in
  /// its inbox here.
  Future<void> _sendQueuedFetch() =>
      _fetchSend ??= _sendFetch().whenComplete(() => _fetchSend = null);

  Future<void> _sendFetch() async {
    if (!me.fetchQueued || !session.connected) return;
    if (atHome) {
      await me.setFetchQueued(false);
      return;
    }
    final here = session.address;
    if (here == null || session.fetchWaiting != null || me.home == null) {
      return;
    }
    await me.setFetchQueued(false);
    session.fetchNews = null;
    await messenger.fetch(here);
    session.note('Fetch: sent, queued while no node was linked');
  }

  /// The queued fetch being sent, which a guest's errand waits for.
  Future<void>? _fetchSend;
  void _queuedFetch() => unawaited(_sendQueuedFetch());

  static const _oldHomeKeeps =
      'Your old home keeps your mail until you stop it there.';

  /// Makes the connected node, at `a`, the person's home: it is given the
  /// mailbox key and is the home only if it then says it holds one. The
  /// new card goes to every contact if its address changed. Returns what
  /// to tell the person.
  Future<String> makeHome(Address a) async {
    final n = session.node;
    if (n == null) return 'Not connected to a node';
    final h = await session.writeHome(me.keys.mailbox.extractBytes());
    if (!h.mine) {
      return 'The node refused. Nothing changed.';
    }
    final away = me.home != null && !atHome;
    if (await me.setHome(a)) await messenger.sendCardToAll();
    return away
        ? 'This node is your home. $_oldHomeKeeps'
        : 'This node is your home';
  }

  /// No home node. Connected to the home, it forgets the mailbox key;
  /// elsewhere the home keeps it, and the person is told.
  Future<String> noHome() async {
    if (me.home == null) return 'No home node';
    final at = atHome;
    final h = at ? await session.writeHome(null) : null;
    if (h != null && h.mine) {
      return 'The node refused. It is still your home.';
    }
    if (await me.setHome(null)) await messenger.sendCardToAll();
    return at ? 'No home node' : 'No home node. $_oldHomeKeeps';
  }

  /// Takes the mailbox key from a connected node that is not the home.
  Future<String> clearKey() async {
    final h = await session.writeHome(null);
    return h.mine
        ? 'The node refused. It still keeps your mail.'
        : 'It no longer keeps your mail';
  }

  void setFake(bool v) {
    fake = v;
    if (!v) {
      fakeNode?.dispose();
      fakeNode = null;
    }
    notifyListeners();
  }

  /// Scans for nodes, after asking for Bluetooth if it is not allowed
  /// yet: the person's tap is where the app asks, and the search runs
  /// from then on.
  Future<void> scan() async {
    if (!bluetoothAllowed) {
      try {
        await _requestBluetooth();
      } finally {
        await checkBluetooth();
      }
    }
    found.clear();
    scanning = true;
    _scanEnded = false;
    _scanLong = false;
    _scanTimer?.cancel();
    _scanTimer = Timer(seenWait, () {
      _scanLong = scanning;
      notifyListeners();
    });
    notifyListeners();
    await _searchStopped();
    try {
      await startScan((n) {
        found[n.deviceId] = n;
        _heard([n]);
        notifyListeners();
      });
    } catch (e) {
      scanning = false;
      _scanTimer?.cancel();
      notifyListeners();
      rethrow;
    }
  }

  Future<void> stopScan() async {
    await stopBleScan();
    _scanTimer?.cancel();
    _scanEnded = scanning || _scanEnded;
    scanning = false;
    notifyListeners();
  }

  /// Whether `device` is the one connected.
  bool connectedTo(String device) => session.connected && linkDevice == device;

  /// The person connects to `n`, after closing the link to another
  /// node, or a guest's to `n`, and pairs with it if the phone has not,
  /// removing any bond Android holds with it first; `again` after they
  /// chose to pair again, which finds the node in a scan first ([seen]),
  /// as Android may have dropped the bond the node refused, and with it
  /// what it knew of the address. It ends the search for the paired node.
  /// Nothing while another connect of the person's is under way, or once
  /// `n` is connected as the phone's: a second connect to the same node
  /// would close the link the first made. A connect that ends with no
  /// link says why: by what it throws, or, where app and node do not serve
  /// each other, by the session's notice.
  Future<void> connect(FoundNode n, {bool again = false}) async {
    if (_tapHeld || (connectedTo(n.deviceId) && !session.guest)) return;
    await _byTap(() async {
      if (connectedTo(n.deviceId) && !session.guest) return;
      await _connect(n, again: again);
    });
  }

  /// The person connects to `n`, a public node, as a guest: without
  /// pairing, so that it stays its owner's. It ends a search round, as
  /// [connect] does, and the link lasts until the node ends it, after a
  /// guest's 30 s, or the person leaves, not one errand (roadmap D179).
  Future<void> connectGuest(FoundNode n) async {
    if (_tapHeld || connectedTo(n.deviceId)) return;
    await _byTap(() => _connectGuest(n, errand: false));
  }

  /// Runs the connect `f` the person asked for, holding [connecting]
  /// until it ends, so that the search neither scans nor connects
  /// meanwhile: a round started when the tap closed the old link took
  /// the home before the tap's own connect was made (bench, 2026-11-10).
  /// A connect of the search's already under way ends first: the tap
  /// waits for it rather than doing nothing.
  Future<void> _byTap(Future<void> Function() f) async {
    _tapHeld = true;
    _tapped = true;
    notifyListeners();
    try {
      await _searchStopped();
      await _linkingEnded();
      await f();
    } finally {
      _tapHeld = false;
      notifyListeners();
    }
  }

  /// Completes once no connection is being set up ([linking]).
  Future<void> _linkingEnded() async {
    if (linking == Linking.none) return;
    final done = Completer<void>();
    void check() {
      if (linking == Linking.none && !done.isCompleted) done.complete();
    }

    addListener(check);
    await done.future;
    removeListener(check);
  }

  /// A connect the person asked for is under way ([_byTap]).
  bool _tapHeld = false;

  /// A guest's errand at `n`: connect without pairing, write the tag and
  /// read the ring's items for the person ([Session.attach]), send what
  /// waits while Ready comes, and leave, as the node shares its one guest
  /// link among every guest in reach (spec "Phone link", Public); not an
  /// `errand` but the person's tap, the link stays until it ends. A node
  /// Android lists a bond with is connected over it first ([_overBond]),
  /// and used as a guest only once it refused the bond and Android has
  /// removed it, after a scan, as the bond went with what Android knew of
  /// the address's type.
  Future<void> _connectGuest(FoundNode n, {bool errand = true}) async {
    final bonded = await BleLink.bonded(n.deviceId);
    // A connect of the person's that came meanwhile goes first.
    if (errand && _tapHeld) return;
    if (bonded) {
      if (await _overBond(n)) return;
      if (!await seen(n.deviceId)) throw NotSeen(shownName(n.name));
    }
    await _open(n.deviceId, n.name, () async {
      await _searchStopped();
      await stopScan();
      return BleLink.connect(n.deviceId, name: n.name, guest: true);
    }, guest: true);
    if (!session.connected || !session.guest) return;
    _guestAt = n.name;
    _guestSeen = (
      found: n,
      address: session.address,
      homeable: canBeHome(session.status),
      room: session.node?.room,
    );
    if (session.guestRead) _guestChecked[n.name] = now();
    if (!errand) return;
    final link = session.link;
    await _sendQueuedFetch();
    await _sent();
    // Only the errand's own link: a tap may have replaced it meanwhile.
    if (session.connected && session.link == link) await session.disconnect();
  }

  /// Completes once nothing waits to be offered to the node, or the link
  /// has ended: the node ends a guest's after 30 s, or at once when
  /// another guest has it, and the next round tries again.
  Future<void> _sent() async {
    final done = Completer<void>();
    void check() {
      if (!done.isCompleted && (!session.connected || !session.toSend)) {
        done.complete();
      }
    }

    session.addListener(check);
    check();
    await done.future;
    session.removeListener(check);
  }

  /// When each public node was last read through, by name.
  final Map<String, DateTime> _guestChecked = {};

  /// The search's rounds in a row that have reached none of the paired
  /// nodes.
  int _ownMissed = 0;

  /// The public node of the last errand, while no round has found none
  /// and the phone has not reached a paired node since.
  String? _guestAt;

  /// The paired nodes are out of reach: [ownGone] rounds in a row have
  /// reached none, and the phone uses public nodes as a guest meanwhile.
  bool get ownAway => pairedNodes.isNotEmpty && _ownMissed >= ownGone;

  /// Whether the search goes on errands at public nodes: without a paired
  /// node, or while they are out of reach.
  bool get usesPublic => pairedNodes.isEmpty || ownAway;

  /// The public node the phone uses as a guest, between its errands too:
  /// the node of the last, while the search runs; or the node of a guest's
  /// link the person made by a tap, while it is up.
  String? get guestNode =>
      tappedGuest || (searchOn && !_tapped) ? _guestAt : null;

  /// The link up is a guest's the person made by a tap, which the node
  /// ends after a guest's 30 s (spec "Phone link", Public).
  bool get tappedGuest => _tapped && session.connected && session.guest;

  /// The last errand's node as it read it.
  GuestNode? _guestSeen;

  /// [guestNode] as its last errand read it, for making it the home or
  /// pairing with it between errands.
  GuestNode? get guestHere {
    final g = _guestSeen;
    return g != null && g.found.name == guestNode ? g : null;
  }

  /// The node the phone uses as a guest is the person's home, as after
  /// they unpaired from their home: the node keeps the mailbox key.
  bool get guestAtHome {
    final a = guestHere?.address;
    return a != null && messenger.isHome(a.id);
  }

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

  /// Whether a public node is due a guest's errand: something waits to be
  /// sent, or its ring has not been read for [guestCheck].
  bool _guestDue(FoundNode n) {
    if (session.toSend) return true;
    final at = _guestChecked[n.name];
    return at == null || now().difference(at) >= guestCheck;
  }

  Future<void> _connect(FoundNode n, {bool again = false}) async {
    final s = session;
    await _open(n.deviceId, n.name, () async {
      await _searchStopped();
      await stopScan();
      final ours =
          s.pairedWith.contains(n.name) && !s.pairingLost.contains(n.name);
      // A node not the phone's is paired with anew, and never over a bond
      // Android holds with it: Android pairs with its old keys while the
      // node runs a new pairing, and the key check fails whatever code is
      // typed; a link over it, to a node that no longer knows it, the node
      // ends before a pairing can start (bench, 2026-11-10). So the bond
      // goes first, as on Forget; a node that still held it takes the new
      // pairing in its place (spec "Phone link", Pairing).
      final bonded = !ours && await BleLink.bonded(n.deviceId);
      if (bonded) {
        if (!await BleLink.forget(n.deviceId)) throw BondKept(n.name);
        s.note('Pairing: the phone forgot its old bond with ${n.name}');
      }
      if (bonded || again) {
        if (!await seen(n.deviceId)) throw NotSeen(shownName(n.name));
      }
      return BleLink.connect(
        n.deviceId,
        name: n.name,
        hadBond: ours,
        pairing: () {
          _phase(Linking.pairing);
          session.note('Pairing with ${n.name}');
        },
        paired: () {
          _phase(Linking.paired);
          session.note('Paired with ${n.name}');
        },
      );
    }, again: () => connect(n, again: true));
  }

  Future<void> connectFake() async {
    if (connecting || connectedTo(fakeDevice)) return;
    final f = fakeNode ??= FakeNode(
      now: now,
      sodium: sodium,
      app: me.identity,
      mailbox: me.keys.mailbox,
    );
    await _open(
      fakeDevice,
      nodeName(f.nodeId),
      () async => (f.connect(), attMtu),
      again: () async {
        f.pair();
        await connectFake();
      },
    );
  }

  /// Connects by `open` and attaches the link, which ends a stop of the
  /// search the person made. Unless the link is a `guest`'s, the node
  /// holds this phone's bond then and is kept as the paired node; one
  /// that no longer does is kept as lost, with `again` to pair with it
  /// anew, and [NotOurs] goes on to the caller; one refused for a bond
  /// the node no longer has is kept in [staleBonds], and [StaleBond] goes
  /// on. An owner's link to `name` that ended while it was read is not
  /// taken as the node holding the bond: [LinkEnded] goes on.
  Future<void> _open(
    String device,
    String name,
    Future<(NodeLink, int)> Function() open, {
    Future<void> Function()? again,
    bool guest = false,
  }) async {
    _phase(Linking.connecting);
    pairAgain = null;
    try {
      await session.setStopped(false);
      if (session.connected) await session.disconnect();
      linkDevice = device;
      final (link, m) = await open();
      _phase(Linking.attaching);
      mtu = m;
      session.note('Connected, MTU $m');
      if (staleBonds.containsValue(device)) {
        staleBonds.removeWhere((_, d) => d == device);
        notifyListeners();
      }
      if (await session.attach(link, guest: guest) && !guest) {
        if (!session.connected) throw LinkEnded(name);
        if (ownAway) session.note('Back on a paired node');
        _ownMissed = 0;
        _guestAt = null;
        await session.setPaired(nodeName(session.node!.id));
      }
    } on NotOurs catch (e) {
      await session.setPaired(e.node, lost: true);
      pairAgain = again;
      rethrow;
    } on StaleBond catch (e) {
      staleBonds[e.node] = device;
      session.note('${e.node} refused this phone\'s old bond');
      rethrow;
    } finally {
      _phase(Linking.none);
    }
  }

  void _phase(Linking l) {
    linking = l;
    notifyListeners();
  }

  /// The person closes the link: the app does not look for the node
  /// again until they connect.
  Future<void> disconnect() async {
    final stop = stopLooking();
    await session.disconnect();
    await stop;
  }

  /// The person stops the search for the paired node, until they connect.
  Future<void> stopLooking() => session.setStopped(true);

  /// Whether the search has started, which only the app on a phone does:
  /// a test starts it to drive it.
  bool _searchStarted = false;

  /// Bluetooth is on, by Android's word.
  bool bluetoothOn = false;
  StreamSubscription<AvailabilityState>? _availability;

  /// The search runs: a round scans, or waits for the next.
  bool _searching = false;

  /// The round under way, which ends once its scan has stopped.
  Future<void>? _round;

  /// Completed to end the search's scan or wait at once.
  Completer<void> _wake = Completer<void>();

  /// The last failure the search noted, so that a failure every round
  /// is told once.
  String? _searchFailed;

  /// The nodes this phone is paired with that still hold its bond; one
  /// that no longer does is the person's to decide on, with Pair again.
  Set<String> get pairedNodes =>
      session.pairedWith.difference(session.pairingLost);

  /// The home's node, by name, if one is set.
  String? get homeName {
    final h = me.home;
    return h == null ? null : nodeName(h.id);
  }

  /// The paired node the search looks for first: the home when the phone
  /// is paired with it, else the one connected last.
  String? get firstPaired {
    final p = pairedNodes;
    final h = homeName;
    return h != null && p.contains(h) ? h : p.lastOrNull;
  }

  /// Whether the search runs: not once the person stopped it, nor with the
  /// fake node in use, nor on a paired node, nor on a guest's link the
  /// person made by a tap ([tappedGuest]), nor while unpairing.
  bool get searchOn {
    if (!_searchStarted || fake || session.stopped || _unpairing) return false;
    return !session.connected || (session.guest && !_tapped);
  }

  /// The paired node the search looks for first, while it runs
  /// ([firstPaired]); null without one, when it looks for public nodes
  /// alone.
  String? get searchFor => searchOn ? firstPaired : null;

  /// Whether the search scans now: not before Bluetooth is allowed, nor
  /// while it is off, a connection is being made, or the person scans.
  bool get lookingNow => searchOn && _mayScan;

  bool get _mayScan =>
      bluetoothAllowed && bluetoothOn && !connecting && !scanning;

  /// The home, while the phone is on another of its paired nodes by the
  /// search's own connect: the one node above it in the order, which a
  /// look-around listens for (roadmap D172). None on a link the person
  /// made by a tap, which is theirs until it drops, nor once they stopped
  /// the search.
  String? get _above {
    if (!_searchStarted || fake || session.stopped || _unpairing) return null;
    final n = session.connected && !session.guest ? session.node : null;
    final h = homeName;
    if (n == null || _tapped || h == null || !pairedNodes.contains(h)) {
      return null;
    }
    return nodeName(n.id) == h ? null : h;
  }

  /// The phone, on a paired node, moves to the home once it hears it
  /// ([_above]).
  bool get movesHome => _above != null;

  /// The last round heard a paired node, too weak to take ([chooseNode]),
  /// and the phone uses a public node meanwhile.
  bool ownWeak = false;

  /// Whether the phone looks around now, from a paired node for the home
  /// ([_above]).
  bool get lookingAround => _above != null && _mayScan;

  /// The link in use was made by the person's tap, not by the search.
  bool _tapped = false;

  /// Whether the link in use may be closed with nothing lost: nothing in
  /// flight on it ([Session.inFlight]) and no firmware update under way
  /// ([Updater.underWay]).
  bool get settled => !session.inFlight && !updater.underWay;

  /// Whether Android lets the app use Bluetooth, as last checked: a fresh
  /// install has not been allowed yet, and its search would only fail.
  bool bluetoothAllowed = false;
  final Future<bool> Function() _bluetoothAllowed;

  /// Checks whether Bluetooth is allowed: at start, when the app comes to
  /// the front and when Bluetooth's state changes, as the person may have
  /// allowed it in Android's settings meanwhile.
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

  /// The paired node the search is scanning for first now.
  String? get lookingFor => lookingNow ? firstPaired : null;

  /// Starts the search, which then runs whenever it should: at start, and
  /// after a link drops.
  void startSearch() {
    if (_searchStarted) return;
    _searchStarted = true;
    _availability = UniversalBle.availabilityStream.listen((a) {
      bluetoothOn = a == AvailabilityState.poweredOn;
      notifyListeners();
      unawaited(checkBluetooth());
    });
    unawaited(checkBluetooth());
    notifyListeners();
  }

  /// What the search does now: its rounds, a look-around from a paired
  /// node, or nothing.
  _Searching? get _doing => lookingNow
      ? _Searching.rounds
      : lookingAround
      ? _Searching.around
      : null;
  _Searching? _did;

  /// Starts the search when it should run, and ends a scan or wait under
  /// way when what it should do changes: a link that drops starts a round
  /// at once, and one that comes up ends the round's wait.
  void _search() {
    final d = _doing;
    if (d != _did) {
      _did = d;
      if (!_wake.isCompleted) _wake.complete();
    }
    if (d == null || _searching) return;
    _searching = true;
    _wake = Completer<void>();
    unawaited(_rounds());
  }

  /// Waits `d`, or until [_wake] completes.
  Future<void> _sleep(Duration d) async {
    final t = Timer(d, () {
      if (!_wake.isCompleted) _wake.complete();
    });
    await _wake.future;
    t.cancel();
  }

  /// When the search last started a scan, a round's or a look-around's.
  DateTime? _scanned;

  /// Each round scans, then connects to the node [chooseNode] picks from
  /// what it heard: the home, another paired node, a node Android holds a
  /// bond with that the app does not count as paired ([_bondedNode]), or
  /// the strongest public node while the phone has no paired node or they
  /// are out of reach ([ownGone]).
  Future<void> _rounds() async {
    try {
      while (_awake()) {
        if (!lookingNow) {
          await _lookAround();
          continue;
        }
        final start = _scanned = now();
        final paired = pairedNodes;
        final round = _listen(_stopAt(paired));
        _round = round;
        final heard = await round;
        _round = null;
        await recheckStale();
        // A round cut short by the person's scan or tap heard too little
        // to call anything out of reach.
        final looked = lookingNow;
        final n = looked
            ? chooseNode(
                heard.values,
                home: homeName,
                paired: pairedNodes,
                lost: await _lostBonded(heard.values),
                stale: staleBonds.keys.toSet(),
                on: session.connected ? _nodeOn : null,
              )
            : null;
        final mine = n != null && pairedNodes.contains(n.name);
        if (mine) await _connectFound(n);
        final bonded = looked && !mine ? await _bondedNode(heard.values) : null;
        if (bonded != null) await _adopt(bonded);
        if (looked &&
            paired.isNotEmpty &&
            !(session.connected && !session.guest) &&
            !_installWaits) {
          _ownMissed++;
          if (_ownMissed == ownGone) {
            session.note(
              'Not reached ${paired.join(', ')} in $ownGone rounds: out of '
              'reach, using public nodes as a guest',
            );
          }
        }
        if (looked && n == null) _guestAt = null;
        // A paired node heard and passed over was heard weak: the public
        // node goes before it at once, as no later round is needed to
        // tell that it is there.
        final weakOwn = heard.values.any((h) => paired.contains(h.name));
        if (looked) ownWeak = weakOwn && !mine;
        if (n != null &&
            !mine &&
            bonded == null &&
            (usesPublic || weakOwn) &&
            _guestDue(n)) {
          await _connectPublic(n);
        }
        if (!_awake() || !lookingNow) continue;
        final left = lookRound - now().difference(start);
        if (left > Duration.zero) await _sleep(left);
      }
    } finally {
      _searching = false;
    }
  }

  /// The nodes no longer this phone's that were `heard` and that Android
  /// still lists the bond with, which they refuse; without the bond each
  /// serves the phone as a guest like any public node.
  Future<Set<String>> _lostBonded(Iterable<FoundNode> heard) async => {
    for (final n in heard)
      if (session.pairingLost.contains(n.name) &&
          await BleLink.bonded(n.deviceId))
        n.name,
  };

  /// The name of the node the phone has a link to.
  String? get _nodeOn {
    final n = session.node;
    return n == null ? null : nodeName(n.id);
  }

  /// A look-around from a paired node: once [lookAround] has passed since
  /// the search's last scan, it listens for the home, and moves to it when
  /// it is heard usable ([chooseNode]) and the link in use is [settled];
  /// one that is not waits for the next.
  Future<void> _lookAround() async {
    final last = _scanned;
    final left = last == null
        ? Duration.zero
        : lookAround - now().difference(last);
    if (left > Duration.zero) return _sleep(left);
    final home = _above!;
    _scanned = now();
    final round = _listen((n) => n.name == home && n.usable);
    _round = round;
    final heard = await round;
    _round = null;
    final on = _nodeOn;
    if (!lookingAround || on == null) return;
    final n = chooseNode(
      heard.values,
      home: homeName,
      paired: pairedNodes,
      on: on,
    );
    if (n == null) return;
    if (!settled) {
      session.note('Heard ${n.name}; moving to it once nothing is in flight');
      return;
    }
    session.note('Heard ${n.name}, ${n.rssi ?? '?'} dBm: moving to it');
    await _connectFound(n);
  }

  /// What ends a round's scan early, as no node heard after it would go
  /// before it: the home heard usable, when the phone is paired with it,
  /// else the one node of `paired` heard usable; nothing for several, of
  /// which the strongest goes first.
  bool Function(FoundNode)? _stopAt(Set<String> paired) {
    final h = homeName;
    final at = h != null && paired.contains(h)
        ? h
        : paired.length == 1
        ? paired.single
        : null;
    return at == null ? null : (n) => n.name == at && n.usable;
  }

  /// Whether the search goes on, its rounds or a look-around, with a new
  /// [_wake] when it does: the last may have ended a scan or wait for a
  /// change that has passed.
  bool _awake() {
    final on = _doing != null;
    if (on && _wake.isCompleted) _wake = Completer<void>();
    return on;
  }

  Future<Map<String, FoundNode>> _listen(
    bool Function(FoundNode)? until,
  ) async {
    try {
      final heard = await listen(until: until, stop: _wake.future);
      _heard(heard.values);
      _searchFailed = null;
      return heard;
    } catch (e) {
      final f = 'Looking for nodes failed: $e';
      if (f != _searchFailed) session.note(f);
      _searchFailed = f;
      return const {};
    }
  }

  /// The node heard strongest of those Android lists a bond with that the
  /// app does not count as paired, as after its data was cleared or on a
  /// new install: the node may still hold the phone's bond. None that
  /// refused the bond already, nor while a link is up.
  Future<FoundNode?> _bondedNode(Iterable<FoundNode> heard) async {
    if (session.connected) return null;
    final byRssi = [...heard]
      ..sort((a, b) => (b.rssi ?? noRssi).compareTo(a.rssi ?? noRssi));
    for (final n in byRssi) {
      if (session.pairedWith.contains(n.name)) continue;
      if (staleBonds.containsKey(n.name)) continue;
      if (await BleLink.bonded(n.deviceId)) return n;
    }
    return null;
  }

  /// Connects to `n`, whose bond Android lists, over that bond, as its
  /// owner: a link over a bond is never a guest's, as the node takes it as
  /// its owner's or ends it. A node that still holds the bond, its first
  /// read on the encrypted link succeeding, is kept among the paired
  /// nodes. One that refuses it has Android remove the bond; where Android
  /// keeps it, [staleBonds] keeps the node, the person is told, and
  /// [StaleBond] goes on. Returns false only once the bond is gone, when
  /// the node may serve the phone as a guest.
  Future<bool> _overBond(FoundNode n) async {
    try {
      await _open(n.deviceId, n.name, () async {
        await _searchStopped();
        await stopScan();
        try {
          return await BleLink.connect(n.deviceId, name: n.name, hadBond: true);
        } on NotOurs {
          throw StaleBond(n.name);
        }
      });
    } on StaleBond {
      if (!await BleLink.forget(n.deviceId)) rethrow;
      staleBonds.remove(n.name);
      session.note('Pairing: the phone forgot its old bond with ${n.name}');
      notifyListeners();
      return false;
    }
    // A node whose version does not serve this app's, as the session's
    // notice says, is not used as a guest either.
    if (!session.connected || session.guest) return true;
    session.note('Pairing: ${n.name} still holds this phone\'s bond');
    adopted = n.name;
    notifyListeners();
    return true;
  }

  /// The search found `n`, whose bond Android holds: taken back as a
  /// paired node, or, refused and its bond removed, used as a guest when
  /// public, after a scan, as the bond went with what Android knew of the
  /// address's type ([_overBond]). Nothing while a connect of the
  /// person's is under way, which holds ([_byTap]).
  Future<void> _adopt(FoundNode n) async {
    if (connecting) return;
    session.note('Found ${n.name}, bonded: connecting as its owner');
    _tapped = false;
    try {
      if (await _overBond(n)) return;
    } on StaleBond {
      return;
    } catch (e) {
      session.note('Connecting to ${n.name} failed: $e');
      return;
    }
    if (n.public && await seen(n.deviceId)) await _connectPublic(n);
  }

  /// The node the search found still paired with this phone though the
  /// app knew of no pairing, until the person makes it their home or
  /// declines: the first screen offers it as the home while none is set.
  String? adopted;

  /// Whether the first screen offers the node the phone is on as the
  /// home, while none is set: the connected node, [adopted], or the one it
  /// uses as a guest ([guestHere]), once it has a place and if it can be a
  /// home. Not again in the app's run once the person said not now.
  bool get offerHome {
    if (_homeNotNow || me.home != null) return false;
    final s = session;
    final n = s.connected && !s.guest ? s.node : null;
    if (n != null) {
      return nodeName(n.id) == adopted &&
          s.address != null &&
          canBeHome(s.status);
    }
    final g = guestHere;
    return g != null && g.address != null && g.homeable && g.room != 0;
  }

  /// The person declined the offer of the home in this run of the app.
  bool _homeNotNow = false;

  /// The person declines the offer of the home.
  void notHome() {
    _homeNotNow = true;
    adopted = null;
    notifyListeners();
  }

  /// Makes the node the phone is on the person's home ([makeHome]). A
  /// public node it uses as a guest is paired with first, by the code its
  /// screen shows ([connect]), as only the paired phone may give a node
  /// the mailbox key (spec "Phone link", Pairing); a pairing that fails
  /// throws, and the phone goes on using the node as a guest. Returns what
  /// to tell the person.
  Future<String> homeHere() async {
    final g = guestHere;
    if (g != null && g.room == 0) return fullNode;
    if (g != null) await connect(g.found);
    final s = session;
    final a = s.address;
    if (!s.connected || s.guest || a == null) {
      return 'Not paired, so not your home';
    }
    if (!canBeHome(s.status)) {
      return 'Paired, but a home must stay on power in one place';
    }
    return makeHome(a);
  }

  /// Connects to the public node the search found, as a guest; a failure
  /// waits for the next round, which takes the strongest again. Nothing
  /// while a connect of the person's is under way ([_byTap]).
  Future<void> _connectPublic(FoundNode n) async {
    if (connecting) return;
    session.note(
      'Found public ${n.name}, ${n.rssi ?? '?'} dBm, connecting '
      '${session.toSend ? 'to send' : 'for mail'}',
    );
    _tapped = false;
    try {
      await _connectGuest(n);
    } catch (e) {
      session.note('Connecting to ${n.name} failed: $e');
    }
  }

  /// Connects to the paired node the search found, by the person's path.
  /// A node no longer this phone's leaves the paired nodes the search
  /// looks for ([pairedNodes]); another failure waits for the next round.
  /// Nothing while a connect of the person's is under way ([_byTap]).
  Future<void> _connectFound(FoundNode n) async {
    if (connecting) return;
    session.note('Found ${n.name}, connecting');
    _tapped = false;
    try {
      await _connect(n);
    } on NotOurs {
      return;
    } catch (e) {
      session.note('Connecting to ${n.name} failed: $e');
    }
  }

  /// Ends the search's scan and waits until it has stopped, so that a
  /// scan or connect of the person's does not meet it.
  Future<void> _searchStopped() async {
    if (!_wake.isCompleted) _wake.complete();
    await _round;
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
    fakeNode?.dispose();
    updater.dispose();
    releases.dispose();
    session.dispose();
    contacts.dispose();
    me.dispose();
    super.dispose();
  }
}

/// What the search does ([AppModel._doing]).
enum _Searching { rounds, around }
