// Small pieces the screens share.

import 'package:flutter/material.dart';

import '../app.dart';
import '../ble_link.dart';
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
String stateLabel(Session s, Outgoing o) => switch (o.state) {
  OutState.queued => 'waiting on this phone',
  OutState.writing => 'being handed to the node',
  OutState.taken => 'sent into the network',
  OutState.givenBack => switch (s.waitLeftS(o)) {
    0 => 'network busy, try again now',
    final w => 'network busy, try again in ${clock(w)}',
  },
  OutState.refused => 'refused: ${o.error}',
};

/// The states of a sent text, in the order it moves through them, each
/// with its icon, for the sheet a tap on a state opens.
const List<(IconData, String, String)> sentStates = [
  (
    Icons.schedule,
    'Waiting on this phone',
    'No node is linked, or the node is not ready for it yet.',
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
    'Network busy',
    'The network could not take it now and gave it back. Try again once '
        'the wait it asked for is over.',
  ),
];

/// Explains a sent text's states in words.
Future<void> explainStates(BuildContext context) => showModalBottomSheet<void>(
  context: context,
  showDragHandle: true,
  builder: (context) => SafeArea(
    child: ListView(
      shrinkWrap: true,
      children: [
        for (final (icon, title, words) in sentStates)
          ListTile(
            leading: Icon(icon),
            title: Text(title),
            subtitle: Text(words),
          ),
      ],
    ),
  ),
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
    if (problem != null) 'part ${problem.part} ${stateLabel(s, problem)}',
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
  final n = app.searchFor ?? 'a public node';
  if (!app.bluetoothOn) return 'Bluetooth is off: turn it on to reach $n.';
  return app.scanning ? null : 'Looking for $n…';
}

/// The phone's pairing as a person reads it while not connected (spec
/// "Phone link", Pairing).
String pairedLine(Session s) {
  final n = s.pairedWith;
  if (n == null) {
    return 'Not paired with a node yet: the first connection pairs, by a '
        'code on the node\'s screen.';
  }
  return s.pairingLost
      ? '$n is no longer yours: another phone has paired with it.'
      : 'Paired with $n.';
}

/// Asks whether `n`, a public node, is the person's: true pairs with it,
/// by the code its screen shows; false uses it as a guest; null decides
/// nothing (spec "Phone link", Pairing, Public).
Future<bool?> askIsYours(BuildContext context, FoundNode n) => showDialog<bool>(
  context: context,
  builder: (context) => SimpleDialog(
    title: Text('Is ${n.name} yours?'),
    children: [
      SimpleDialogOption(
        onPressed: () => Navigator.pop(context, true),
        child: const ListTile(
          title: Text('Pair'),
          subtitle: Text('You will type the code its screen shows.'),
        ),
      ),
      SimpleDialogOption(
        onPressed: () => Navigator.pop(context, false),
        child: const ListTile(
          title: Text('Just use it'),
          subtitle: Text('It passes on your texts; it stays its owner\'s.'),
        ),
      ),
    ],
  ),
);

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
