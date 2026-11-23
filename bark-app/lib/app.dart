// What every screen shares, made of objects each with one job, which
// take what they use by their constructors: the person (`me.dart`), their
// contacts, the session with the node, the messages sealed and opened
// (`messenger.dart`), how the phone reaches a node over Bluetooth
// (`links.dart`), the home and its owner (`mailbox.dart`), moving to a new
// phone (`moving.dart`), a node's install (`installs.dart`), and the
// updates of the node and the app. The model composes them, and says
// what the link's notification and the first screen say of them.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';

import 'alerts.dart';
import 'app_update.dart';
import 'contacts.dart';
import 'identity_file.dart';
import 'installs.dart';
import 'l10n.dart';
import 'links.dart';
import 'link.dart';
import 'mailbox.dart';
import 'me.dart';
import 'messenger.dart';
import 'moving.dart';
import 'phone_notifier.dart';
import 'phone_place.dart';
import 'releases.dart';
import 'session.dart';
import 'store.dart';
import 'updater.dart';

class AppModel {
  final Sodium sodium;

  final Me me;
  final Session session;
  final Contacts contacts;
  late final Messenger messenger;
  final DateTime Function() now;
  final Notifier notifier;

  /// The identity file and the 12 words: moving to another phone.
  late final Moving moving;

  /// The person's home and its owner.
  late final Mailbox mailbox;

  /// A node's install, which the search waits for.
  late final Installs installs = Installs(
    session: session,
    clock: now,
    situation: () => links.situation(),
  );

  /// How the phone reaches a node over Bluetooth.
  late final Links links;
  late final Alerts alerts;

  /// The latest release, against this app and the connected node.
  final Releases releases;

  /// The download and install of a newer app.
  late final AppUpdate appUpdate = AppUpdate(apks, notifier.installApk)
    ..noted = session.note;

  /// Where a newer app's APK is kept.
  final Apks apks;

  /// Where the phone is, asked only on the person's tap.
  final PhonePlace place;

  AppModel({
    required this.sodium,
    required this.me,
    required this.session,
    required this.contacts,
    required this.now,
    this.notifier = const NoNotifier(),
    Releases? releases,
    this.apks = const NoApks(),
    PasswordKey passwordKey = argon2id,
    this.place = const AndroidPlace(),
    Future<bool> Function()? bluetoothAllowed,
    Future<void> Function()? requestBluetooth,
  }) : releases = releases ?? Releases(MemoryStore('release'), now: now) {
    this.releases.noted = session.note;
    moving = Moving(
      sodium: sodium,
      passwordKey: passwordKey,
      me: me,
      contacts: contacts,
      session: session,
    );
    messenger = Messenger(sodium, me, contacts, session, now);
    // First of the session's listeners, so that the card's node is kept
    // before anything else hears of the link.
    mailbox = Mailbox(
      sodium: sodium,
      session: session,
      me: me,
      contacts: contacts,
      messenger: messenger,
      now: now,
      guestHere: () => links.guestHere,
      connect: (n) => links.connect(n),
    );
    // Made before the links, so that the node's reboot is waited for
    // before the search hears its link ended.
    updater.away = installs.rebooted;
    links = Links(
      session: session,
      me: me,
      installs: installs,
      now: now,
      updating: () => updater.underWay,
      sendQueuedFetch: mailbox.sendQueuedFetch,
      bluetoothAllowed: bluetoothAllowed,
      requestBluetooth: requestBluetooth,
    );
    alerts = Alerts(notifier, contacts);
    session.kept = (r) =>
        alerts.kept(r, session.partsOf(r)).catchError(_unnotified);
    // The link's notification says what each of them says of the link.
    for (final n in [session, me, installs, links]) {
      n.addListener(_link);
    }
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
      apks: ApkDir(),
    );
  }

  /// The check for a release, due a day after the last.
  Timer? _releaseCheck;

  void _checkWhenDue() {
    _releaseCheck?.cancel();
    final at = releases.dueAt;
    if (at == null) return;
    final wait = at.difference(now());
    _releaseCheck = Timer(wait.isNegative ? Duration.zero : wait, () async {
      await releases.check();
      _checkWhenDue();
    });
  }

  /// The firmware update of the connected node.
  late final Updater updater = Updater(
    session,
    () => links.mtu ?? defaultAttMtu,
    release: () => releases.latest,
  );

  /// Everything a screen redraws on.
  Listenable get changes => Listenable.merge([
    links,
    mailbox,
    installs,
    session,
    contacts,
    me,
    updater,
    releases,
    appUpdate,
  ]);

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
    _checkWhenDue();
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
    final p = links.firstPaired;
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
        body: session.connected ? tr.notifyTextsArrive : tr.notifyChecksMinute,
      );
    }
    if (!links.searchOn || !links.bluetoothAllowed) return null;
    return _looking(sought, bluetooth: links.bluetoothOn);
  }

  static Held _looking(String node, {required bool bluetooth}) => (
    title: searchLine(node, bluetooth: bluetooth),
    body: bluetooth ? tr.notifyOutInReach : tr.notifyOutBluetooth,
  );

  /// The line while the phone is on no node and looks for `node`, by
  /// Bluetooth or not.
  static String searchLine(String node, {required bool bluetooth}) =>
      bluetooth ? tr.statusLooking(node) : tr.statusBluetoothOff(node);

  /// The line while the phone is on no node and does not look: the person
  /// stopped the search, or it waits for Bluetooth to be allowed.
  static String get notLookingLine => tr.statusNotLooking;

  /// The node the search looks for, as a person reads it: the first of
  /// the paired nodes ([firstPaired]), else a public node.
  String get sought {
    final f = links.searchFor;
    return f == null ? tr.statusPublicNode : shownName(f);
  }

  void _unnotified(Object e) => session.note('Notifying failed: $e');

  /// Sets the person's name, which their card carries and the node they
  /// are paired with lists for its other phones.
  Future<void> setName(String n) async {
    await me.setName(n);
    await session.nameChanged();
  }

  /// The card changed and has not gone to the contacts. Without contacts
  /// there is no one to send it to.
  bool get cardUnsent => me.cardUnsent && contacts.list.isNotEmpty;

  /// Why the card changed, for the line that offers to send it.
  String get cardChange =>
      me.home == null ? tr.commonCardNoHome : tr.commonCardNewHome;

  /// Asks, once, to notify; `why` shows the person why first and says
  /// whether to go on.
  Future<void> askToNotify(Future<bool> Function() why) async {
    if (me.askedToNotify) return;
    await me.setAskedToNotify();
    if (await notifier.allowed()) return;
    if (await why()) await notifier.ask();
  }

  /// The steps to a first text, each done or not, for the first screen's
  /// card until all are done: a name for the card, a node, by the search
  /// or paired, and a contact.
  ({bool name, bool node, bool contact}) get gettingStarted => (
    name: me.name.isNotEmpty,
    node:
        session.connected ||
        session.nodes.paired.isNotEmpty ||
        me.nodeAddress != null,
    contact: contacts.list.isNotEmpty,
  );

  /// Which node the phone is on, as the first screen, Settings and the
  /// link's notification say it; null while it is on none. A public node
  /// is on between errands too. Why it is on that node is the help's to
  /// say, not the line's.
  String? get nodeLine {
    final n = session.connected ? session.node : null;
    final g = n != null && session.guest ? nodeName(n.id) : links.guestNode;
    final h = links.homeName;
    // The home is a sentence of its own after the line.
    final home = h == null ? '' : '. ${tr.statusHomeAt(shownName(h))}';
    final installs = this.installs.note;
    if (g != null && links.tappedGuest) {
      return '${tr.statusGuestFor30s(shownName(g), installs)}$home';
    }
    if (g != null && (n == null || session.guest)) {
      return '${tr.statusGuest(shownName(g), installs)}$home';
    }
    if (n == null) return null;
    if (mailbox.atHome) return tr.statusAtHome(nodeLabel(n.id), installs);
    return '${tr.statusPaired(nodeLabel(n.id), installs)}$home';
  }

  void dispose() {
    _releaseCheck?.cancel();
    for (final n in [session, me, installs, links]) {
      n.removeListener(_link);
    }
    links.dispose();
    mailbox.dispose();
    installs.dispose();
    updater.dispose();
    releases.dispose();
    appUpdate.dispose();
    session.dispose();
    contacts.dispose();
    me.dispose();
  }
}
