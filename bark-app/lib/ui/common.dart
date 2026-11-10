// Small pieces the screens share.

import 'package:flutter/material.dart';

import '../app.dart';
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
  OutState.refused => 'refused: ${o.error}',
};

/// `s` with its first letter a capital.
String capital(String s) =>
    s.isEmpty ? s : '${s[0].toUpperCase()}${s.substring(1)}';

/// The states of a sent text, in the order it moves through them, each
/// with its icon, for the sheet a tap on a state opens.
const List<(IconData, String, String)> sentStates = [
  (
    Icons.schedule,
    'Waiting for a node',
    'It waits on this phone: no node is linked, or the node is not ready '
        'for it yet.',
  ),
  (
    Icons.sync,
    'Being handed to the node',
    'The phone is giving it to the node.',
  ),
  (
    Icons.check,
    'Sent into the network',
    'The node has it and passes it on, hop by hop. The network gives no '
        'further news: it does not say when it arrives.',
  ),
  (
    Icons.done_all,
    'Confirmed by the recipient',
    'They tapped Got it, and their confirmation came back.',
  ),
  (
    Icons.hourglass_bottom,
    'The network is busy',
    'The network could not take it now and gave it back, with a wait. '
        'The app sends it again by itself once the wait is over.',
  ),
];

/// What each mark on a received text means, for the sheet a tap on its
/// marks opens; checked against the spec's "Payload" and "Mailbox"
/// sections and the parts of a long text (roadmap D137).
const List<(IconData, String, String)> receivedMarks = [
  (
    Icons.check,
    'Got it',
    'Tap it to tell the sender you read it. The app sends them a short '
        'confirmation of its own, and their copy shows two checks once it '
        'arrives. Nothing is confirmed by itself.',
  ),
  (
    Icons.check,
    'You tapped Got it',
    'The app sent the sender your confirmation. Like any text, it may '
        'take a while to reach them.',
  ),
  (
    Icons.gpp_maybe,
    'First message',
    'It came as a first message, in the form anyone who holds your card '
        'can make, so it may not be theirs. Texts from them since came in '
        'the form only they can make; this one is not proven.',
  ),
  (
    Icons.markunread_mailbox,
    'From your mailbox',
    'It waited at your home node while your phone was away, and came '
        'when the app fetched it.',
  ),
  (
    Icons.more_horiz,
    'Parts',
    'A long text comes in up to 16 parts, each a message of its own, '
        'which may arrive in any order. A line in italics stands for a '
        'part not yet here.',
  ),
  (
    Icons.schedule,
    'The time',
    'When it came to this phone. When the sender wrote it, by their '
        'phone\'s clock, is under This message.',
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
bool partsAgree(List<Outgoing> outs) => outs.every(
  (o) => o.state == outs.first.state && o.acked == outs.first.acked,
);

/// A sent text's parts' states in words when they differ: how many the
/// node has taken, how many are confirmed, and the first problem.
String partsLabel(Session s, List<Outgoing> outs) {
  final taken = outs.where((o) => o.acked || o.state == OutState.taken);
  final acked = outs.where((o) => o.acked);
  final problem = outs
      .where(
        (o) =>
            !o.acked &&
            (o.state == OutState.givenBack || o.state == OutState.refused),
      )
      .firstOrNull;
  return [
    '${taken.length} of ${outs.length} sent',
    if (acked.isNotEmpty) '${acked.length} confirmed',
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

/// While a node refuses the phone's old bond ([AppModel.staleBond]): the
/// line that says so, with the way to the phone's Bluetooth settings.
class StaleBondLine extends StatelessWidget {
  final AppModel app;
  const StaleBondLine(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    final n = app.staleBond;
    if (n == null) return const SizedBox.shrink();
    return Card(
      color: Theme.of(context).colorScheme.errorContainer,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(staleBondLine(n)),
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
    '${e is NotPaired ? '$e' : 'Pairing failed: $e'}. The phone still uses '
    'the node as a guest.';

/// While no home is set and the phone is on a node that can be one
/// ([AppModel.offerHome]), the node the search found still paired with the
/// phone or a public node it uses as a guest: makes it the home in one tap,
/// pairing first with a guest's.
class HomeOfferLine extends StatelessWidget {
  final AppModel app;
  const HomeOfferLine(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    if (!app.offerHome) return const SizedBox.shrink();
    final guest = app.guestHere != null;
    final card = app.contacts.list.isEmpty
        ? ''
        : ' Your new card then goes to your contacts.';
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              guest
                  ? 'This phone has no home node set. As your home, this '
                        'node keeps texts to you and gives your card a fixed '
                        'address. The phone pairs with it first: you type '
                        'the code its screen shows.$card'
                  : app.contacts.list.isEmpty
                  ? 'This phone has no home node set.'
                  : 'This phone has no home node set. Making this node '
                        'your home sends your new card to your contacts.',
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
                            if (context.mounted) {
                              say(
                                context,
                                guest
                                    ? notPairedLine(e)
                                    : 'Setting the home failed: $e',
                              );
                            }
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
            : 'Your $n contacts have the old one; one message each.',
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

/// The search for the paired node, or a public node, while it scans or
/// waits for Bluetooth; null when there is none.
String? lookingLine(AppModel app) {
  if (!app.searchOn || !app.bluetoothAllowed || app.connecting) return null;
  final p = app.searchFor;
  final n = p == null ? 'a public node' : shownName(p);
  if (!app.bluetoothOn) return 'Bluetooth is off: turn it on to reach $n.';
  return app.scanning ? null : 'Looking for $n…';
}

/// Which public node the phone uses as a guest, and why, during an errand
/// there and between them; null while it uses none. A paired phone does
/// so while its node is out of reach (spec "Phone link", Which node a
/// phone uses).
String? guestLine(AppModel app) {
  final s = app.session;
  return s.connected && !s.guest ? null : app.nodeLine;
}

/// The first screen's line on the phone's node: which it is on, and why.
String connectionLine(AppModel app) {
  final looking = lookingLine(app);
  return app.nodeLine ??
      (looking != null
          ? '$looking Texts wait on the phone until it is in reach.'
          : 'Not connected to a node: texts wait on the phone. '
                '${pairedLine(app.session)}');
}

/// The phone's pairing as a person reads it while not connected (spec
/// "Phone link", Pairing).
String pairedLine(Session s) {
  final p = s.pairedWith;
  if (p == null) {
    return 'Not paired with a node yet: the phone uses a public node near '
        'you as a guest.';
  }
  final n = shownName(p);
  return s.pairingLost
      ? '$n is no longer yours: another phone has paired with it.'
      : 'Paired with $n.';
}

/// Says why bark wants Bluetooth before Android asks; whether to go on.
Future<bool> whyBluetooth(BuildContext context) async =>
    await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Bluetooth'),
        content: const Text(
          'bark sends and receives through a node near you, which it finds '
          'and talks to over Bluetooth. Android asks you next.',
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

/// Strength, in dBm, at or above which a node's advertisement reads as
/// strong. A choice: these are the advertisement's strength as the phone
/// hears it before any link, so the node's noise floor, by which a link's
/// quality is judged, does not apply.
const int strongDbm = -70;

/// Strength, in dBm, below which a node's advertisement reads as weak; a
/// choice, as [strongDbm].
const int weakDbm = -85;

/// A found node's signal in a word: strong, fair or weak.
String signalWord(int? rssi) => rssi == null
    ? 'signal unknown'
    : rssi >= strongDbm
    ? 'strong signal'
    : rssi < weakDbm
    ? 'weak signal'
    : 'fair signal';
