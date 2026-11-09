// One conversation: its texts in order and a field to write the next;
// an archived one read-only, with a button to bring it back.

import 'package:flutter/material.dart';

import '../app.dart';
import '../app_layer.dart';
import '../contacts.dart';
import '../conversations.dart';
import '../session.dart';
import 'archived_screen.dart';
import 'common.dart';

/// The mark on a received text that came as a first message: only a box
/// proves its sender, so a verified thread may still hold such a text.
const unverifiedText = 'Came as a first message, which anyone could have made';

class ThreadScreen extends StatefulWidget {
  final AppModel app;

  /// The other person's identity in hex.
  final String keyHex;
  const ThreadScreen(this.app, this.keyHex, {super.key});

  @override
  State<ThreadScreen> createState() => _ThreadScreenState();
}

class _ThreadScreenState extends State<ThreadScreen> {
  final text = TextEditingController();

  AppModel get app => widget.app;

  @override
  void initState() {
    super.initState();
    app.alerts.opened(widget.keyHex);
  }

  @override
  void dispose() {
    app.alerts.closed(widget.keyHex);
    text.dispose();
    super.dispose();
  }

  /// Sends the text where the contact is while that is fresh; when it
  /// may be stale, with a home, where the person chooses, this once;
  /// without one, there after saying it is old, as nothing else is
  /// (roadmap D132, D162).
  Future<void> _send(Contact to) async {
    if (text.text.isEmpty) return;
    final now = app.now();
    final options = to.towards(now);
    final stale = to.stale(now);
    final ask =
        options.length > 1 ||
        (options.length == 1 && stale != null && to.card.home == null);
    final toward = !ask
        ? options.firstOrNull
        : await showDialog<Toward>(
            context: context,
            builder: (context) => SimpleDialog(
              title: const Text('Send to'),
              children: [
                if (to.card.home == null && stale != null)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
                    child: Text(
                      'Last seen there ${ago(stale)}: it keeps their mail '
                      'only while they come back',
                    ),
                  ),
                for (final t in options)
                  SimpleDialogOption(
                    onPressed: () => Navigator.pop(context, t),
                    child: ListTile(
                      title: Text(_option(to, t)),
                      subtitle: Text(switch (t) {
                        Toward.there => 'Fast if they are still near there',
                        Toward.home =>
                          'Waits at their home until they fetch it',
                      }),
                    ),
                  ),
              ],
            ),
          );
    if (toward == null) return;
    try {
      await app.messenger.sendText(to, text.text, toward: toward);
      text.clear();
    } on BadPayload catch (e) {
      if (mounted) say(context, 'Not sent: $e');
    }
  }

  String _option(Contact c, Toward t) => switch (t) {
    Toward.there =>
      'Where ${c.name} was ${ago(app.now().difference(c.placeAt!))}',
    Toward.home => 'Their home mailbox',
  };

  /// When they last wrote, and whether from their home.
  String _heard(Contact c) {
    final at = c.heardAt;
    if (at == null) return 'Has not written yet';
    final from = c.place;
    final home = c.card.home;
    final where = home == null || from == null
        ? ''
        : from.toString() == home.toString()
        ? ', from their home'
        : ', away from home';
    return 'Last wrote ${ago(app.now().difference(at))}$where';
  }

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final c = thread(widget.keyHex, app.contacts.list, s.outbox, s.received);
    final contact = c.contact;
    final archived = app.contacts.isArchived(widget.keyHex);
    return Scaffold(
      appBar: AppBar(
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(c.title, overflow: TextOverflow.ellipsis),
            if (contact != null)
              Text(
                _heard(contact),
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
          ],
        ),
        actions: [
          FetchButton(app),
          if (!archived)
            PopupMenuButton<void Function()>(
              onSelected: (f) => f(),
              itemBuilder: (_) => [
                if (contact != null) ...[
                  PopupMenuItem(
                    value: () => _name(contact),
                    child: const Text('Rename'),
                  ),
                  PopupMenuItem(
                    value: () => app.messenger.sendCard(contact),
                    child: const Text('Send my card'),
                  ),
                ],
                PopupMenuItem(
                  value: () => _archive(
                    app.contacts.archiveConversation(widget.keyHex),
                    '${c.title}: conversation archived',
                  ),
                  child: const Text('Archive conversation'),
                ),
                if (contact != null) ...[
                  PopupMenuItem(
                    value: () => _archive(
                      app.contacts.archiveContact(contact),
                      '${contact.name}: contact archived',
                    ),
                    child: const Text('Archive contact'),
                  ),
                  PopupMenuItem(
                    value: () async {
                      if (await confirm(
                        context,
                        'Remove ${contact.name}?',
                        'Their card goes from this phone. Their texts stay, '
                            'but you cannot write to them until you add '
                            'their card again.',
                        'Remove',
                      )) {
                        await app.contacts.remove(contact);
                      }
                    },
                    child: const Text('Remove contact'),
                  ),
                ],
              ],
            ),
        ],
      ),
      body: SafeArea(
        top: false,
        child: Column(
          children: [
            NoticeCard(app),
            if (contact != null && contact.unverified)
              const Padding(
                padding: EdgeInsets.fromLTRB(16, 8, 16, 0),
                child: Text(
                  'Not verified yet: a first message can come from anyone. '
                  'Once a reply of theirs opens here, it is theirs.',
                ),
              ),
            if (!app.me.gotItHinted &&
                !archived &&
                c.entries.any(
                  (e) => !e.mine && !e.arrived.every((r) => r.acked),
                ))
              const Padding(
                padding: EdgeInsets.fromLTRB(16, 8, 16, 0),
                child: Text('Tap Got it so the sender sees you read it.'),
              ),
            Expanded(
              child: ListView(
                reverse: true,
                padding: const EdgeInsets.all(8),
                children: [
                  for (final e in c.entries.reversed)
                    _bubble(
                      context,
                      e,
                      !(contact?.unverified ?? false),
                      archived,
                    ),
                ],
              ),
            ),
            if (archived)
              Padding(
                padding: const EdgeInsets.all(16),
                child: Row(
                  children: [
                    const Expanded(
                      child: Text('This conversation is archived.'),
                    ),
                    FilledButton.icon(
                      icon: const Icon(Icons.unarchive),
                      label: const Text('Unarchive'),
                      onPressed: () => app.contacts.unarchive(widget.keyHex),
                    ),
                  ],
                ),
              )
            else if (contact == null)
              const Padding(
                padding: EdgeInsets.all(16),
                child: Text('This contact was removed.'),
              )
            else if (contact.towards(app.now()).isEmpty)
              // Only a card from an app before D162 names no node at all.
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  '${contact.name}\'s card is from an older app and names '
                  'no node: ask them for a new card.',
                ),
              )
            else
              _compose(contact),
          ],
        ),
      ),
    );
  });

  /// Archives, and leaves the thread, which the first screen no longer
  /// shows; the undo is said there.
  Future<void> _archive(Future<void> done, String what) async {
    final m = ScaffoldMessenger.of(context);
    await done;
    if (!mounted) return;
    Navigator.of(context).pop();
    archivedSay(m, app, widget.keyHex, what);
  }

  Future<void> _name(Contact c) async {
    final name = await askText(context, 'Name', c.given);
    if (name == null || name.isEmpty) return;
    await app.contacts.put(c.copyWith(name: name));
  }

  /// How many messages the text takes, once it takes more than one.
  String? _parts(Contact to, String t) {
    final int n;
    try {
      n = app.messenger.partsOf(to, t).length;
    } on BadPayload catch (e) {
      return e.why;
    }
    if (n <= 1) return null;
    return n > maxParts ? '$n parts, $maxParts at most' : '$n parts';
  }

  Widget _compose(Contact to) => Padding(
    padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
    child: Row(
      children: [
        Expanded(
          child: ValueListenableBuilder(
            valueListenable: text,
            builder: (context, v, _) => TextField(
              controller: text,
              minLines: 1,
              maxLines: 5,
              textCapitalization: TextCapitalization.sentences,
              decoration: InputDecoration(
                hintText: 'Text',
                border: const OutlineInputBorder(),
                helperText: _parts(to, v.text),
              ),
            ),
          ),
        ),
        IconButton(
          tooltip: 'Send',
          icon: const Icon(Icons.send),
          onPressed: () => _send(to),
        ),
      ],
    ),
  );

  /// `verified`: the line above the thread does not already say that any
  /// text in it may be anyone's. `archived`: read-only, so nothing in it
  /// sends or drops.
  Widget _bubble(BuildContext context, Entry e, bool verified, bool archived) {
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall!;
    final mine = e.mine;
    final color = scheme.onSurfaceVariant;
    return Align(
      alignment: mine ? Alignment.centerRight : Alignment.centerLeft,
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: MediaQuery.of(context).size.width * 0.8,
        ),
        child: Card(
          color: mine ? scheme.primaryContainer : scheme.surfaceContainerHigh,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (mine)
                  SelectableText(e.text)
                else
                  SelectableText.rich(
                    TextSpan(
                      children: [
                        for (final p in e.pieces)
                          TextSpan(
                            text: p.text,
                            style: p.missing
                                ? TextStyle(
                                    fontStyle: FontStyle.italic,
                                    color: color,
                                  )
                                : null,
                          ),
                      ],
                    ),
                  ),
                const SizedBox(height: 4),
                Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(when(e.at), style: small),
                    if (mine) ..._sentMarks(e.outs, small, scheme),
                    if (!mine)
                      ..._receivedMarks(e, verified, small, color, archived),
                  ],
                ),
                if (mine && !archived) _actions(e.outs),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Where a sent text went and how far it got: one part's marks when the
  /// parts agree, else their states in words.
  List<Widget> _sentMarks(
    List<Outgoing> outs,
    TextStyle small,
    ColorScheme scheme,
  ) {
    final o = outs.first;
    final color = scheme.onSurfaceVariant;
    return [
      if (o.toward != null) _where(o.toward!, small, color),
      if (!partsAgree(outs)) ...[
        const SizedBox(width: 4),
        Flexible(child: Text(partsLabel(app.session, outs), style: small)),
      ] else if (o.acked)
        _stateMark(
          Icons.done_all,
          'Confirmed by the recipient',
          small,
          scheme.primary,
        )
      else
        ..._state(o, small, color),
    ];
  }

  /// Whether a received text came as a first message, where the thread
  /// is verified; whether it came from the mailbox; and its
  /// acknowledgement: a plain check once every part that has come is
  /// acknowledged, else the button, which a part that comes later brings
  /// back.
  List<Widget> _receivedMarks(
    Entry e,
    bool verified,
    TextStyle small,
    Color color,
    bool archived,
  ) => [
    if (verified && e.arrived.any((r) => r.intro))
      _mark(Icons.gpp_maybe, unverifiedText, small, color),
    if (e.arrived.any((r) => r.mailbox))
      _mark(Icons.markunread_mailbox, 'From your mailbox', small, color),
    if (e.arrived.every((r) => r.acked))
      // One plain check: the person knows they have it, so the two
      // coloured ones would say nothing.
      _mark(Icons.check, 'You tapped Got it', small, color)
    else if (!archived)
      _acknowledge(e.arrived.first, small),
  ];

  /// An icon the small text's size, with the words it stands for as its
  /// tooltip and semantic label, so nothing is lost to someone who does
  /// not know the icon.
  Widget _mark(IconData icon, String words, TextStyle small, Color color) =>
      Padding(
        padding: const EdgeInsets.only(left: 4),
        child: Tooltip(
          message: words,
          excludeFromSemantics: true,
          child: Icon(
            icon,
            size: small.fontSize,
            color: color,
            semanticLabel: words,
          ),
        ),
      );

  Widget _where(Toward t, TextStyle small, Color color) => switch (t) {
    Toward.there => _mark(Icons.place, 'To where they were', small, color),
    Toward.home => _mark(Icons.home, 'To their home', small, color),
  };

  /// A sent text's state short of confirmed, each its own icon (common.dart
  /// [sentStates]). Not yet with the node shows a clock rather than
  /// nothing, so a missing mark never means two things. A state that is a
  /// problem keeps its words, which say why.
  List<Widget> _state(Outgoing o, TextStyle small, Color color) =>
      switch (o.state) {
        OutState.queued => [
          _stateMark(Icons.schedule, 'Waiting for a node', small, color),
        ],
        OutState.writing => [
          _stateMark(Icons.sync, 'Being handed to the node', small, color),
        ],
        OutState.taken => [
          _stateMark(
            Icons.check,
            'Sent into the network; no further news until they confirm',
            small,
            color,
          ),
        ],
        OutState.givenBack => [
          _stateMark(
            Icons.hourglass_bottom,
            'The network is busy',
            small,
            color,
          ),
          const SizedBox(width: 4),
          Flexible(child: Text(stateLabel(app.session, o), style: small)),
        ],
        OutState.refused => [
          const SizedBox(width: 4),
          Flexible(child: Text(stateLabel(app.session, o), style: small)),
        ],
      };

  /// A sent text's state mark, which a tap explains with the others.
  Widget _stateMark(
    IconData icon,
    String words,
    TextStyle small,
    Color color,
  ) => InkWell(
    onTap: () => explainStates(context),
    child: _mark(icon, words, small, color),
  );

  /// A labelled chip, unlike the plain check it turns into, so "tap to
  /// confirm" cannot be mistaken for "confirmed", with a tap target of
  /// 48 dp, the least Material asks for.
  Widget _acknowledge(Received r, TextStyle small) => Padding(
    padding: const EdgeInsets.only(left: 4),
    child: ActionChip(
      avatar: const Icon(Icons.check),
      label: const Text('Got it'),
      tooltip: 'Tell the sender you read it',
      materialTapTargetSize: MaterialTapTargetSize.padded,
      onPressed: () async {
        await app.me.setGotItHinted();
        try {
          await app.messenger.acknowledge(r);
        } on BadPayload catch (e) {
          if (mounted) say(context, 'Not sent: $e');
        }
      },
    ),
  );

  /// Cancel, for the parts not yet with the node; a part given back goes
  /// again by itself once its wait is over.
  Widget _actions(List<Outgoing> outs) {
    final s = app.session;
    final droppable = [
      for (final o in outs)
        if (o.state != OutState.writing && o.state != OutState.taken) o,
    ];
    if (droppable.isEmpty) return const SizedBox.shrink();
    return TextButton(
      onPressed: () async {
        for (final o in droppable) {
          await s.drop(o);
        }
      },
      child: const Text('Cancel'),
    );
  }
}
