// Small pieces the screens share.

import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app.dart';
import '../ble_link.dart' show strongDbm, weakDbm;
import '../card.dart';
import '../l10n.dart';
import '../link.dart';
import '../node_link.dart';
import '../session.dart';

/// Keeps the app's window out of screenshots, screen recordings and the
/// recents thumbnail while it shows `child`, which holds the person's
/// seed or a password that seals it (`FLAG_SECURE`, MainActivity.kt).
class Secret extends StatefulWidget {
  final Widget child;
  const Secret({required this.child, super.key});

  static const channel = MethodChannel('org.bark.bark_app/window');

  /// How many are shown: a dialog over another clears the flag only once
  /// both are gone.
  static int _shown = 0;

  static Future<void> _secure(bool on) async {
    try {
      await channel.invokeMethod<void>('secure', on);
    } on MissingPluginException {
      // No window to protect: the tests, without a phone.
    }
  }

  @override
  State<Secret> createState() => _SecretState();
}

class _SecretState extends State<Secret> {
  @override
  void initState() {
    super.initState();
    if (Secret._shown++ == 0) unawaited(Secret._secure(true));
  }

  @override
  void dispose() {
    if (--Secret._shown == 0) unawaited(Secret._secure(false));
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// The second's tick of what a screen shows that moves with the clock
/// alone, such as "sending again in 0:42" or "20 min ago". It runs only
/// while a [Ticking] is shown and the app is in front, so it wakes nothing
/// else: what changes the model's state at a set time has its own timer.
class Seconds extends ChangeNotifier {
  Seconds._();
  static final Seconds instance = Seconds._();

  Timer? _timer;
  bool _front = true;

  /// The app's window is in front; set by the app as that changes.
  set front(bool f) {
    _front = f;
    _run();
  }

  @override
  void addListener(VoidCallback listener) {
    super.addListener(listener);
    _run();
  }

  @override
  void removeListener(VoidCallback listener) {
    super.removeListener(listener);
    _run();
  }

  void _run() {
    if (hasListeners && _front) {
      _timer ??= Timer.periodic(
        const Duration(seconds: 1),
        (_) => notifyListeners(),
      );
    } else {
      _timer?.cancel();
      _timer = null;
    }
  }
}

/// Builds `builder` again every second ([Seconds]) while `when` holds as
/// it is built, else once.
class Ticking extends StatelessWidget {
  final bool when;
  final WidgetBuilder builder;
  const Ticking(this.builder, {this.when = true, super.key});

  @override
  Widget build(BuildContext context) => when
      ? ListenableBuilder(
          listenable: Seconds.instance,
          builder: (c, _) => builder(c),
        )
      : builder(context);
}

void say(BuildContext context, String s) =>
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s)));

String clock(int s) => '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';

/// Hours and minutes, or the date too when not today.
String when(DateTime t) {
  final l = t.toLocal();
  final n = DateTime.now();
  String two(int v) => v.toString().padLeft(2, '0');
  final hm = '${two(l.hour)}:${two(l.minute)}';
  if (l.year == n.year && l.month == n.month && l.day == n.day) return hm;
  return '${l.year}-${two(l.month)}-${two(l.day)} $hm';
}

/// How long ago, in a few words.
String ago(Duration d) {
  if (d.inMinutes < 1) return tr.timeJustNow;
  if (d.inHours < 1) return tr.timeMinutesAgo(d.inMinutes);
  if (d.inDays < 2) return tr.timeHoursAgo(d.inHours);
  return tr.timeDaysAgo(d.inDays);
}

/// What a sent text's state means to the person, in words that claim no
/// more than the app knows: once the node has taken a message the network
/// says nothing more until the recipient confirms (spec "Addresses").
/// Lower case, as it follows other words; [capital] for a line of its own.
String stateLabel(Session s, Outgoing o) => switch (o.state) {
  OutState.queued => tr.sentQueued,
  OutState.writing => tr.sentWriting,
  OutState.taken => tr.sentTaken,
  OutState.givenBack => switch (s.waitLeftS(o)) {
    0 => tr.sentBusyNow,
    final w => tr.sentBusyIn(clock(w)),
  },
  OutState.refused => tr.sentRefused,
};

/// The day a record's person was last seen, as a node counts it
/// ([dayOf]), in words against `now`: "today", "yesterday", "N days ago",
/// or "never" when the node had no time at any of its links.
String lastSeen(int day, DateTime now) {
  final d = dayOf(day);
  if (d == null) return tr.timeNever;
  final t = now.toUtc();
  final ago = DateTime.utc(t.year, t.month, t.day).difference(d).inDays;
  return switch (ago) {
    <= 0 => tr.timeToday,
    1 => tr.timeYesterday,
    _ => tr.timeDaysAgo(ago),
  };
}

/// `s` with its first letter a capital.
String capital(String s) =>
    s.isEmpty ? s : '${s[0].toUpperCase()}${s.substring(1)}';

/// The states of a sent text, in the order it moves through them, each
/// with its icon, and the reaction it may get, for the sheet a tap on the
/// text opens.
List<(IconData, String, String)> get sentStates => [
  (Icons.schedule, capital(tr.sentQueued), tr.sentQueuedHelp),
  (Icons.sync, capital(tr.sentWriting), tr.sentWritingHelp),
  (Icons.check, capital(tr.sentTaken), tr.sentTakenHelp),
  (Icons.hourglass_bottom, tr.sentBusy, tr.sentBusyHelp),
  (Icons.block, capital(tr.sentRefused), tr.sentRefusedHelp),
  (Icons.add_reaction_outlined, tr.sentReaction, tr.sentReactionHelp),
];

/// What each mark on a received text means, for the sheet a tap on its
/// marks opens; checked against the spec's "Payload" and "Mailbox"
/// sections and the parts of a long text (roadmap D137).
List<(IconData, String, String)> get receivedMarks => [
  (
    Icons.add_reaction_outlined,
    tr.receivedYourReaction,
    tr.receivedYourReactionHelp,
  ),
  (Icons.gpp_maybe, tr.receivedFirst, tr.receivedFirstHelp),
  (Icons.markunread_mailbox, tr.receivedMailbox, tr.receivedMailboxHelp),
  (Icons.more_horiz, tr.receivedParts, tr.receivedPartsHelp),
  (Icons.schedule, tr.receivedTime, tr.receivedTimeHelp),
];

/// A message's details that its bubble does not show, as label and
/// value, then what each of its marks means.
Future<void> explainMessage(
  BuildContext context, {
  required List<(String, String)> details,
  required List<(IconData, String, String)> marks,
}) => showModalBottomSheet<void>(
  context: context,
  showDragHandle: true,
  isScrollControlled: true,
  builder: (context) {
    final text = Theme.of(context).textTheme;
    return SafeArea(
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (details.isNotEmpty) ...[
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 4),
                child: Text(tr.commonThisMessage, style: text.titleMedium),
              ),
              for (final (label, value) in details)
                Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 16,
                    vertical: 4,
                  ),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(label),
                      const SizedBox(width: 16),
                      Expanded(
                        child: SelectableText(value, textAlign: TextAlign.end),
                      ),
                    ],
                  ),
                ),
              const Divider(),
            ],
            for (final (icon, title, words) in marks)
              ListTile(
                leading: Icon(icon),
                title: Text(title),
                subtitle: Text(words),
              ),
          ],
        ),
      ),
    );
  },
);

/// Whether a sent text's parts are all in one state, so that one part's
/// marks stand for them all.
bool partsAgree(List<Outgoing> outs) =>
    outs.every((o) => o.state == outs.first.state);

/// A sent text's parts' states in words when they differ: how many the
/// node has taken and the first problem.
String partsLabel(Session s, List<Outgoing> outs) {
  final taken = outs.where((o) => o.state == OutState.taken);
  final problem = outs
      .where(
        (o) => o.state == OutState.givenBack || o.state == OutState.refused,
      )
      .firstOrNull;
  return [
    tr.sentPartsSent(taken.length, outs.length),
    if (problem != null)
      tr.sentPartProblem(problem.part, stateLabel(s, problem)),
  ].join(', ');
}

/// A sent text's state in words, one part's or the parts'.
String textLabel(Session s, List<Outgoing> outs) =>
    partsAgree(outs) ? stateLabel(s, outs.first) : partsLabel(s, outs);

/// Rebuilds on every change of the model, the session and the contacts.
class Watch extends StatelessWidget {
  final AppModel app;
  final WidgetBuilder builder;
  const Watch(this.app, this.builder, {super.key});

  @override
  Widget build(BuildContext context) =>
      ListenableBuilder(listenable: app.changes, builder: (c, _) => builder(c));
}

/// The session's last notice, dismissible.
class NoticeCard extends StatelessWidget {
  final AppModel app;
  const NoticeCard(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    final n = app.session.notice;
    if (n == null) return const SizedBox.shrink();
    return Card(
      color: Theme.of(context).colorScheme.errorContainer,
      child: ListTile(
        title: Text(n.text),
        trailing: IconButton(
          icon: const Icon(Icons.close),
          onPressed: app.session.dismissNotice,
        ),
      ),
    );
  }
}

/// While Android keeps a bond the app asked it to remove
/// ([AppModel.bondKept]): the line that says so, with the way to the
/// phone's Bluetooth settings.
class BondKeptLine extends StatelessWidget {
  final AppModel app;
  const BondKeptLine(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    final n = app.links.bondKept;
    if (n == null) return const SizedBox.shrink();
    return Card(
      color: Theme.of(context).colorScheme.errorContainer,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(bondKeptLine(n)),
            Align(
              alignment: Alignment.centerRight,
              child: BluetoothSettingsButton(app),
            ),
          ],
        ),
      ),
    );
  }
}

/// What the person reads when pairing with a node the phone uses as a
/// guest failed for `e`: why, and that the phone goes on as before.
String notPairedLine(Object e) =>
    e is NotPaired ? tr.commonNotPaired('$e') : tr.commonPairingFailed('$e');

/// While no home is set and the phone uses a public node as a guest that
/// can be one ([AppModel.offerHome]): makes it the home in one tap,
/// pairing first.
/// The homes contacts gave the person, or only `of`'s, each with **Use
/// it** and **Not now** (`README.md`, "A home given").
class GiftLines extends StatelessWidget {
  final AppModel app;
  final String? of;
  const GiftLines(this.app, {this.of, super.key});

  @override
  Widget build(BuildContext context) => Column(
    children: [
      for (final c in app.contacts.list)
        if (c.gift case final g? when of == null || of == c.key)
          Card(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(tr.commonGift(c.name, nodeLabel(g.home.id))),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      HelpButton(tr.commonGiftTitle, tr.commonGiftHelp),
                      TextButton(
                        onPressed: () => app.mailbox.declineGift(c),
                        child: Text(tr.notNow),
                      ),
                      TextButton(
                        onPressed: () async {
                          try {
                            final r = await app.mailbox.useGift(c);
                            if (context.mounted) say(context, r);
                          } catch (e) {
                            if (context.mounted) {
                              say(context, tr.commonFailed('$e'));
                            }
                          }
                        },
                        child: Text(tr.commonUseGift),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
    ],
  );
}

class HomeOfferLine extends StatelessWidget {
  final AppModel app;
  const HomeOfferLine(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    if (!app.mailbox.offerHome) return const SizedBox.shrink();
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(tr.commonHomeOffer),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TextButton(
                  onPressed: app.mailbox.notHome,
                  child: Text(tr.notNow),
                ),
                TextButton(
                  onPressed: app.links.connecting
                      ? null
                      : () async {
                          try {
                            final r = await app.mailbox.homeHere();
                            if (context.mounted) say(context, r);
                          } catch (e) {
                            if (context.mounted) say(context, notPairedLine(e));
                          }
                        },
                  child: Text(tr.commonSetHome),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Opens the phone's Bluetooth settings, where the person forgets a node.
class BluetoothSettingsButton extends StatelessWidget {
  final AppModel app;
  const BluetoothSettingsButton(this.app, {super.key});

  @override
  Widget build(BuildContext context) => TextButton(
    onPressed: () => openBluetoothSettings(context, app),
    child: Text(tr.commonBluetoothSettings),
  );
}

Future<void> openBluetoothSettings(BuildContext context, AppModel app) async {
  try {
    await app.notifier.openBluetoothSettings();
  } catch (e) {
    if (context.mounted) say(context, tr.commonBluetoothSettingsFailed('$e'));
  }
}

/// Tells the person `text`, which asks them to forget a node in the
/// phone's Bluetooth settings, with the way there.
Future<void> askToForget(BuildContext context, AppModel app, String text) =>
    showDialog<void>(
      context: context,
      builder: (dialog) => AlertDialog(
        content: Text(text),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialog),
            child: Text(tr.notNow),
          ),
          FilledButton(
            onPressed: () {
              Navigator.pop(dialog);
              openBluetoothSettings(context, app);
            },
            child: Text(tr.commonBluetoothSettings),
          ),
        ],
      ),
    );

/// "Fetch my mail", in an app bar, while the person has a home node; it
/// says what was done, or why nothing was sent. While a fetch waits for
/// its answer the button is off and says so; "No new mail", or how many
/// more the home holds, is told once, and the count stays on the button
/// until a fetch brings them.
class FetchButton extends StatefulWidget {
  final AppModel app;
  const FetchButton(this.app, {super.key});

  @override
  State<FetchButton> createState() => _FetchButtonState();
}

class _FetchButtonState extends State<FetchButton> {
  AppModel get app => widget.app;
  Listenable? _changes;
  bool _current = true;

  @override
  void initState() {
    super.initState();
    _changes = app.changes..addListener(_tell);
  }

  @override
  void didUpdateWidget(FetchButton old) {
    super.didUpdateWidget(old);
    if (old.app == app) return;
    _changes?.removeListener(_tell);
    _changes = app.changes..addListener(_tell);
  }

  // Told again as the screen above this one closes.
  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _current = ModalRoute.of(context)?.isCurrent != false;
    _tell();
  }

  @override
  void dispose() {
    _changes?.removeListener(_tell);
    super.dispose();
  }

  /// Says the home's news once, on the screen in front, after the frame:
  /// another screen's button would take it unseen.
  void _tell() {
    if (!_current || app.me.home == null || app.session.fetchNews == null) {
      return;
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final news = app.session.fetchNews;
      if (!mounted || !_current || app.me.home == null || news == null) {
        return;
      }
      app.session.fetchNews = null;
      say(context, news);
    });
  }

  // The wait counts down, and ends by the clock.
  @override
  Widget build(BuildContext context) =>
      Ticking(when: app.session.fetchWaiting != null, _button);

  Widget _button(BuildContext context) {
    if (app.me.home == null) return const SizedBox.shrink();
    final s = app.session;
    if (app.me.fetchQueued) {
      return IconButton(
        tooltip: tr.commonFetchQueued,
        icon: const Icon(Icons.schedule_send),
        onPressed: null,
      );
    }
    final waiting = s.fetchWaiting;
    if (waiting != null) {
      return IconButton(
        tooltip: tr.commonFetchWaiting(clock(s.fetchWaitLeftS(waiting))),
        icon: const Icon(Icons.hourglass_top),
        onPressed: null,
      );
    }
    final more = app.me.remaining;
    return IconButton(
      tooltip: tr.commonFetch,
      icon: Badge(
        label: Text('$more'),
        isLabelVisible: more > 0,
        child: const Icon(Icons.move_to_inbox),
      ),
      onPressed: () async {
        try {
          final r = await app.mailbox.fetchNow();
          if (context.mounted) say(context, r);
        } catch (e) {
          if (context.mounted) say(context, tr.commonFetchFailed('$e'));
        }
      },
    );
  }
}

/// While the card has changed and not gone to the contacts, says why, with
/// **Send my card**: the app sends no card by itself (roadmap D126).
class SendCardLine extends StatelessWidget {
  final AppModel app;
  final EdgeInsetsGeometry? padding;
  const SendCardLine(this.app, {this.padding, super.key});

  @override
  Widget build(BuildContext context) {
    if (!app.cardUnsent) return const SizedBox.shrink();
    final n = app.contacts.list.length;
    return ListTile(
      contentPadding: padding,
      leading: const Icon(Icons.badge_outlined),
      title: Text(app.cardChange),
      subtitle: Text(tr.commonCardOld(n)),
      trailing: FilledButton(
        onPressed: () async {
          try {
            await app.messenger.sendCardToAll();
            if (context.mounted) say(context, tr.commonCardQueued);
          } catch (e) {
            if (context.mounted) say(context, tr.commonCardFailed('$e'));
          }
        },
        child: Text(tr.commonSendCard),
      ),
    );
  }
}

/// Asks for the person's name and sets it, as Settings and My card do.
Future<void> askMyName(BuildContext context, AppModel app) async {
  final n = await askText(
    context,
    tr.commonMyName,
    app.me.name,
    maxBytes: PersonCard.maxName,
  );
  if (n != null) await app.setName(n);
}

/// Asks for one line of text; null if the person cancels. A `secret` one,
/// the 12 words, is kept out of screenshots ([Secret]).
Future<String?> askText(
  BuildContext context,
  String label,
  String initial, {
  bool secret = false,
  int? maxBytes,
}) {
  final c = TextEditingController(text: initial);
  return showDialog<String>(
    context: context,
    builder: (context) {
      final ask = AlertDialog(
        // Its field stays in reach above the keyboard on a phone held sideways.
        scrollable: true,
        title: Text(label),
        content: TextField(
          controller: c,
          autofocus: true,
          textCapitalization: TextCapitalization.words,
          // The keyboard keeps no word of a secret.
          enableSuggestions: !secret,
          autocorrect: !secret,
          enableIMEPersonalizedLearning: !secret,
          // What would not fit is not typed, so the person sees the limit.
          inputFormatters: [
            if (maxBytes != null)
              TextInputFormatter.withFunction(
                (was, now) =>
                    utf8.encode(now.text).length <= maxBytes ? now : was,
              ),
          ],
          onSubmitted: (v) => Navigator.pop(context, v.trim()),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(tr.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, c.text.trim()),
            child: Text(tr.save),
          ),
        ],
      );
      return secret ? Secret(child: ask) : ask;
    },
  );
}

/// The search's line while the phone is on no node and looks for its
/// home, a paired node or a public node, or waits for Bluetooth; null
/// when it does not look, while a connection is being made and while the
/// person scans.
String? lookingLine(AppModel app) {
  final l = app.links;
  if (!l.searchOn || !l.bluetoothAllowed || l.connecting) return null;
  if (!app.links.bluetoothOn) {
    return AppModel.searchLine(app.sought, bluetooth: false);
  }
  return app.links.scanning
      ? null
      : AppModel.searchLine(app.sought, bluetooth: true);
}

/// The line on the phone's node, one of four: on its home, on another
/// node it is paired with, on a public node as a guest, or on none, as
/// when a paired node is away to install a new version. Why is
/// [connectionHelp]'s to say.
String connectionLine(AppModel app) =>
    app.nodeLine ??
    app.installs.installingLine ??
    lookingLine(app) ??
    AppModel.notLookingLine;

/// Why the phone is on the node [connectionLine] names, or on none, in
/// a few plain sentences for the person who asks (`bark-app/README.md`,
/// "Screens"; roadmap D163, D171).
String connectionHelp(AppModel app) {
  final s = app.session;
  final lost = [for (final n in app.session.nodes.lost) shownName(n)];
  final lostLine = switch (lost) {
    [] => '',
    [final n] => '\n\n${tr.connectionLostOne(n)}',
    _ => '\n\n${tr.connectionLostMany(lost.join(', '))}',
  };
  final unserved = [
    for (final r in app.session.nodes.records)
      if (r.unserved) shownName(r.name),
  ];
  final unservedLine = switch (unserved) {
    [] => '',
    [final n] => '\n\n${tr.connectionUnservedOne(n)}',
    _ => '\n\n${tr.connectionUnservedMany(unserved.join(', '))}',
  };
  final others = '$lostLine$unservedLine';
  final install = app.installs.installHelp;
  if (install != null) return install;
  if (s.connected && !s.guest) {
    if (app.mailbox.atHome) return tr.connectionAtHome;
    final home = app.links.homeName == null
        ? tr.connectionPairedNoHome
        : app.links.movesHome
        ? '${tr.connectionPairedHome} ${tr.connectionMovesHome}'
        : tr.connectionPairedHome;
    return '${tr.connectionPaired} $home';
  }
  if (app.links.tappedGuest) return '${tr.connectionTappedGuest}$others';
  if (app.nodeLine != null) {
    final why = app.links.ownWeak
        ? tr.connectionGuestWeak
        : app.links.ownAway
        ? tr.connectionGuestAway
        : tr.connectionGuestNoneNear;
    return '$why ${tr.connectionGuestVisits}$others';
  }
  if (lookingLine(app) != null) {
    if (!app.links.bluetoothOn) return '${tr.connectionBluetoothOff}$others';
    if (app.links.searchFor == null) return '${tr.connectionLookPublic}$others';
    return '${tr.connectionLookPaired}$others';
  }
  return '${tr.connectionNotLooking}$others';
}

/// An info button that opens `body` under `title`, for whoever asks.
class HelpButton extends StatelessWidget {
  final String title;
  final String body;
  const HelpButton(this.title, this.body, {super.key});

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: tr.commonAbout(title),
    icon: const Icon(Icons.info_outline),
    onPressed: () => showHelp(context, title, body),
  );
}

/// A sheet of `body` under `title`.
Future<void> showHelp(BuildContext context, String title, String body) =>
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (context) => SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 8),
              Text(body),
            ],
          ),
        ),
      ),
    );

/// The button by [connectionLine] that opens [connectionHelp].
class ConnectionHelpButton extends StatelessWidget {
  final AppModel app;
  const ConnectionHelpButton(this.app, {super.key});

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: tr.commonWhyThisNode,
    icon: const Icon(Icons.info_outline),
    onPressed: () =>
        showHelp(context, connectionLine(app), connectionHelp(app)),
  );
}

/// Says why bark wants Bluetooth before Android asks; whether to go on.
Future<bool> whyBluetooth(BuildContext context) async =>
    await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Bluetooth'),
        content: Text(tr.commonWhyBluetooth),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr.notNow),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(tr.commonContinue),
          ),
        ],
      ),
    ) ??
    false;

/// Asks the person to confirm `action`, which `body` explains; whether
/// they did.
Future<bool> confirm(
  BuildContext context,
  String title,
  String body,
  String action,
) async =>
    await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(title),
        content: Text(body),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(action),
          ),
        ],
      ),
    ) ??
    false;

/// How the connected node fares, in words: still in its first rotation,
/// or how many other nodes it hears (spec "Phone link", Status).
String health(Session s) {
  final left = s.bootLeftS;
  if (left > 0) return tr.statusStarting(clock(left));
  final n = s.status?.heard ?? 0;
  if (n == 0) return tr.statusHearsNone;
  return tr.statusHears(n);
}

/// A found node's signal in a word: strong, fair or weak.
String signalWord(int? rssi) => rssi == null
    ? tr.signalUnknown
    : rssi >= strongDbm
    ? tr.signalStrong
    : rssi < weakDbm
    ? tr.signalWeak
    : tr.signalFair;
