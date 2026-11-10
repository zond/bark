// Small pieces the screens share.

import 'package:flutter/material.dart';

import '../app.dart';
import '../ble_link.dart' show strongDbm, weakDbm;
import '../link.dart';
import '../node_link.dart';
import '../session.dart';

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
  if (d.inMinutes < 1) return 'just now';
  if (d.inHours < 1) return '${d.inMinutes} min ago';
  if (d.inDays < 2) return '${d.inHours} h ago';
  return '${d.inDays} days ago';
}

/// What a sent text's state means to the person, in words that claim no
/// more than the app knows: once the node has taken a message the network
/// says nothing more until the recipient confirms (spec "Addresses").
/// Lower case, as it follows other words; [capital] for a line of its own.
String stateLabel(Session s, Outgoing o) => switch (o.state) {
  OutState.queued => 'waiting for a node',
  OutState.writing => 'being handed to the node',
  OutState.taken => 'sent into the network',
  OutState.givenBack => switch (s.waitLeftS(o)) {
    0 => 'the network is busy, sending again now',
    final w => 'the network is busy, sending again in ${clock(w)}',
  },
  OutState.refused => 'refused by the node',
};

/// The day a node's phone was last linked, as a record counts it
/// ([dayOf]), in words against `now`: "today", "yesterday", "N days ago",
/// or "never" when the node had no time at any of its links.
String lastLinked(int day, DateTime now) {
  final d = dayOf(day);
  if (d == null) return 'never';
  final t = now.toUtc();
  final ago = DateTime.utc(t.year, t.month, t.day).difference(d).inDays;
  return switch (ago) {
    <= 0 => 'today',
    1 => 'yesterday',
    _ => '$ago days ago',
  };
}

/// `s` with its first letter a capital.
String capital(String s) =>
    s.isEmpty ? s : '${s[0].toUpperCase()}${s.substring(1)}';

/// The states of a sent text, in the order it moves through them, each
/// with its icon, and the reaction it may get, for the sheet a tap on the
/// text opens.
const List<(IconData, String, String)> sentStates = [
  (
    Icons.schedule,
    'Waiting for a node',
    'On this phone, until a node is linked and ready for it.',
  ),
  (
    Icons.sync,
    'Being handed to the node',
    'The phone is giving it to the node.',
  ),
  (
    Icons.check,
    'Sent into the network',
    'The node has it and passes it on. The network gives no further '
        'news: it does not say when it arrives.',
  ),
  (
    Icons.hourglass_bottom,
    'The network is busy',
    'The network gave it back for now. The app sends it again by itself '
        'once the wait is over.',
  ),
  (
    Icons.block,
    'Refused by the node',
    'The node will not take it as it is, a fault of the app or the node. '
        'Cancel removes it; Debug shows the reason.',
  ),
  (
    Icons.add_reaction_outlined,
    'Their reaction',
    'The emoji on the text\'s corner, if they picked one.',
  ),
];

/// What each mark on a received text means, for the sheet a tap on its
/// marks opens; checked against the spec's "Payload" and "Mailbox"
/// sections and the parts of a long text (roadmap D137).
const List<(IconData, String, String)> receivedMarks = [
  (
    Icons.add_reaction_outlined,
    'Your reaction',
    'Press and hold a text to react with an emoji, or to copy it. They '
        'see it once it arrives. Pick another to change it.',
  ),
  (
    Icons.gpp_maybe,
    'First message',
    'Anyone with your card could have sent a first message, so this one '
        'is not proven to be theirs.',
  ),
  (
    Icons.markunread_mailbox,
    'From your mailbox',
    'It waited at your home node and came when you fetched your mail.',
  ),
  (
    Icons.more_horiz,
    'Parts',
    'A long text comes in parts, in any order. Italics stand for a part '
        'not yet here.',
  ),
  (
    Icons.schedule,
    'The time',
    'When it came. When it was written is under This message.',
  ),
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
                child: Text('This message', style: text.titleMedium),
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
    '${taken.length} of ${outs.length} sent',
    if (problem != null) 'part ${problem.part}: ${stateLabel(s, problem)}',
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
        title: Text(n),
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
    final n = app.bondKept;
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
    '${e is NotPaired ? '$e' : 'Pairing failed: $e'}. Still a guest here.';

/// While no home is set and the phone uses a public node as a guest that
/// can be one ([AppModel.offerHome]): makes it the home in one tap,
/// pairing first.
class HomeOfferLine extends StatelessWidget {
  final AppModel app;
  const HomeOfferLine(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    if (!app.offerHome) return const SizedBox.shrink();
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'No home node yet. Make this one your home? It keeps texts '
              'to you while you are away. You pair first, with the code on '
              'its screen.',
            ),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TextButton(
                  onPressed: app.notHome,
                  child: const Text('Not now'),
                ),
                TextButton(
                  onPressed: app.connecting
                      ? null
                      : () async {
                          try {
                            final r = await app.homeHere();
                            if (context.mounted) say(context, r);
                          } catch (e) {
                            if (context.mounted) say(context, notPairedLine(e));
                          }
                        },
                  child: const Text('Make this node my home'),
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
    child: const Text('Bluetooth settings'),
  );
}

Future<void> openBluetoothSettings(BuildContext context, AppModel app) async {
  try {
    await app.notifier.openBluetoothSettings();
  } catch (e) {
    if (context.mounted) say(context, 'Opening Bluetooth settings failed: $e');
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
            child: const Text('Not now'),
          ),
          FilledButton(
            onPressed: () {
              Navigator.pop(dialog);
              openBluetoothSettings(context, app);
            },
            child: const Text('Bluetooth settings'),
          ),
        ],
      ),
    );

/// "Fetch my mail", in an app bar, while the person has a home node; it
/// says what was done, or why nothing was sent. While a fetch waits for
/// its answer the button is off and says so; "No new mail", or how many
/// more the home holds, is told once, and the count stays on the button
/// until a fetch brings them.
class FetchButton extends StatelessWidget {
  final AppModel app;
  const FetchButton(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    if (app.me.home == null) return const SizedBox.shrink();
    final s = app.session;
    final news = s.fetchNews;
    if (news != null && ModalRoute.of(context)?.isCurrent != false) {
      s.fetchNews = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (context.mounted) say(context, news);
      });
    }
    if (app.me.fetchQueued) {
      return const IconButton(
        tooltip: 'Fetch waits for a node',
        icon: Icon(Icons.schedule_send),
        onPressed: null,
      );
    }
    final waiting = s.fetchWaiting;
    if (waiting != null) {
      return IconButton(
        tooltip:
            'Waiting for your home node to answer, '
            '${clock(s.fetchWaitLeftS(waiting))} at most',
        icon: const Icon(Icons.hourglass_top),
        onPressed: null,
      );
    }
    final more = app.me.remaining;
    return IconButton(
      tooltip: 'Fetch my mail',
      icon: Badge(
        label: Text('$more'),
        isLabelVisible: more > 0,
        child: const Icon(Icons.move_to_inbox),
      ),
      onPressed: () async {
        try {
          final r = await app.fetchNow();
          if (context.mounted) say(context, r);
        } catch (e) {
          if (context.mounted) say(context, 'Fetching failed: $e');
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
      subtitle: Text(
        n == 1
            ? 'Your contact has the old one.'
            : 'Your $n contacts have the old one.',
      ),
      trailing: FilledButton(
        onPressed: () async {
          try {
            await app.messenger.sendCardToAll();
            if (context.mounted) say(context, 'Your card is queued');
          } catch (e) {
            if (context.mounted) say(context, 'Sending the card failed: $e');
          }
        },
        child: const Text('Send my card'),
      ),
    );
  }
}

/// Asks for the person's name and sets it, as Settings and My card do.
Future<void> askMyName(BuildContext context, AppModel app) async {
  final n = await askText(context, 'My name', app.me.name);
  if (n != null) await app.me.setName(n);
}

/// Asks for one line of text; null if the person cancels.
Future<String?> askText(BuildContext context, String label, String initial) {
  final c = TextEditingController(text: initial);
  return showDialog<String>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(label),
      content: TextField(
        controller: c,
        autofocus: true,
        textCapitalization: TextCapitalization.words,
        onSubmitted: (v) => Navigator.pop(context, v.trim()),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, c.text.trim()),
          child: const Text('Save'),
        ),
      ],
    ),
  );
}

/// The search's line while the phone is on no node and looks for its
/// home, a paired node or a public node, or waits for Bluetooth; null
/// when it does not look, while a connection is being made and while the
/// person scans.
String? lookingLine(AppModel app) {
  if (!app.searchOn || !app.bluetoothAllowed || app.connecting) return null;
  if (!app.bluetoothOn) {
    return AppModel.searchLine(app.sought, bluetooth: false);
  }
  return app.scanning ? null : AppModel.searchLine(app.sought, bluetooth: true);
}

/// The line on the phone's node, one of four: on its home, on another
/// node it is paired with, on a public node as a guest, or on none, as
/// when a paired node is away to install a new version. Why is
/// [connectionHelp]'s to say.
String connectionLine(AppModel app) =>
    app.nodeLine ??
    app.installingLine ??
    lookingLine(app) ??
    AppModel.notLookingLine;

/// Why the phone is on the node [connectionLine] names, or on none, in
/// a few plain sentences for the person who asks (spec "Phone link",
/// Which node a phone uses).
String connectionHelp(AppModel app) {
  final s = app.session;
  final lost = [for (final n in s.pairingLost) shownName(n)];
  final lostLine = switch (lost) {
    [] => '',
    [final n] => '\n\n$n is no longer yours: it no longer knows this phone.',
    _ =>
      '\n\n${lost.join(', ')} are no longer yours: they no longer know this '
          'phone.',
  };
  final install = app.installHelp;
  if (install != null) return install;
  if (s.connected && !s.guest) {
    if (app.atHome) {
      return 'Your home. The phone stays linked while it is in reach, so '
          'texts come at once, also with the screen off. It always goes to '
          'your home first.';
    }
    final home = app.homeName == null
        ? 'With no home, your card names this node while you use it.'
        : 'Texts to you wait at your home; Fetch my mail brings them here.'
              '${app.movesHome ? ' When your home is in reach, the phone moves to it within a minute.' : ''}';
    return 'A node you are paired with. The phone stays linked while it is '
        'in reach. $home';
  }
  if (app.tappedGuest) {
    return 'Someone else\'s public node, which you chose. It serves one '
        'guest at a time and ends a guest\'s link after 30 seconds; then the '
        'phone chooses a node itself.$lostLine';
  }
  if (app.nodeLine != null) {
    final why = app.ownWeak
        ? 'Your paired nodes are heard too weakly here, so the phone uses '
              'this public node until it hears one well.'
        : app.ownAway
        ? 'None of your paired nodes has been in reach for 30 seconds, so '
              'the phone uses this public node until it hears one.'
        : 'No node of yours is near, so the phone uses this public node, '
              'someone else\'s.';
    return '$why As a guest it visits about once a minute, and at once when '
        'you send. Replies to texts sent from here come here.$lostLine';
  }
  if (lookingLine(app) != null) {
    if (!app.bluetoothOn) {
      return 'Bluetooth is off. Texts wait on the phone until it is on and a '
          'node is in reach.$lostLine';
    }
    if (app.searchFor == null) {
      return 'No node of yours to use, so the phone looks every 15 seconds '
          'for a public node to use as a guest.$lostLine';
    }
    return 'The phone looks every 15 seconds for your paired nodes, your '
        'home first. After 30 seconds without one, it uses a public node '
        'as a guest meanwhile.$lostLine';
  }
  return 'The phone is not looking: you stopped it, or it may not use '
      'Bluetooth yet. Tap a node in Settings to connect.$lostLine';
}

/// An info button that opens `body` under `title`, for whoever asks.
class HelpButton extends StatelessWidget {
  final String title;
  final String body;
  const HelpButton(this.title, this.body, {super.key});

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: 'About $title',
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
    tooltip: 'Why this node',
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
        content: const Text(
          'bark texts through a node near you, over Bluetooth. Android asks '
          'next.',
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
            child: const Text('Cancel'),
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
  if (left > 0) return 'Starting up, ${clock(left)}';
  final n = s.status?.heard ?? 0;
  if (n == 0) return 'Hears no other node yet: texts wait here';
  return n == 1 ? 'Hears 1 node' : 'Hears $n nodes';
}

/// A found node's signal in a word: strong, fair or weak.
String signalWord(int? rssi) => rssi == null
    ? 'signal unknown'
    : rssi >= strongDbm
    ? 'strong signal'
    : rssi < weakDbm
    ? 'weak signal'
    : 'fair signal';
