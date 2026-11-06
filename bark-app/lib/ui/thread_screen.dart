// One conversation: its texts in order and a field to write the next.

import 'package:flutter/material.dart';

import '../app.dart';
import '../app_layer.dart';
import '../contacts.dart';
import '../conversations.dart';
import '../session.dart';
import 'common.dart';

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

  /// Sends the text where the contact is, or, when that may be stale,
  /// where the person chooses, this once.
  Future<void> _send(Contact to) async {
    if (text.text.isEmpty) return;
    final options = to.towards(app.now());
    final toward = options.length == 1
        ? options.single
        : await showDialog<Toward>(
            context: context,
            builder: (context) => SimpleDialog(
              title: const Text('Send to'),
              children: [
                for (final t in options)
                  SimpleDialogOption(
                    onPressed: () => Navigator.pop(context, t),
                    child: Text(_option(to, t)),
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
      'Where ${c.name} was ${ago(app.now().difference(c.heardAt!))}',
    Toward.home => 'Their home mailbox',
  };

  /// When they last wrote, and whether from their home.
  String _heard(Contact c) {
    final at = c.heardAt;
    if (at == null) return 'Has not written yet';
    final from = c.wroteFrom;
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
          if (contact != null)
            PopupMenuButton<void Function()>(
              onSelected: (f) => f(),
              itemBuilder: (_) => [
                PopupMenuItem(
                  value: () => _name(contact),
                  child: const Text('Rename'),
                ),
                PopupMenuItem(
                  value: () => app.messenger.sendCard(contact),
                  child: const Text('Send my card'),
                ),
                PopupMenuItem(
                  value: () => app.contacts.remove(contact),
                  child: const Text('Remove contact'),
                ),
              ],
            ),
        ],
      ),
      body: SafeArea(
        top: false,
        child: Column(
          children: [
            NoticeCard(app),
            if (contact != null && !contact.verified)
              const Padding(
                padding: EdgeInsets.fromLTRB(16, 8, 16, 0),
                child: Text(
                  'Not verified yet: a first message can come from anyone. '
                  'Once a reply of theirs opens here, it is theirs.',
                ),
              ),
            Expanded(
              child: ListView(
                reverse: true,
                padding: const EdgeInsets.all(8),
                children: [
                  for (final e in c.entries.reversed) _bubble(context, e),
                ],
              ),
            ),
            if (contact == null)
              const Padding(
                padding: EdgeInsets.all(16),
                child: Text('This contact was removed.'),
              )
            else if (contact.towards(app.now()).isEmpty)
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  '${contact.name} has no home node and has not written yet: '
                  'you can answer once they write to you.',
                ),
              )
            else
              _compose(contact),
          ],
        ),
      ),
    );
  });

  Future<void> _name(Contact c) async {
    final name = await askText(context, 'Name', c.name);
    if (name == null || name.isEmpty) return;
    await app.contacts.put(c.copyWith(name: name));
  }

  Widget _compose(Contact to) => Padding(
    padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
    child: Row(
      children: [
        Expanded(
          child: TextField(
            controller: text,
            minLines: 1,
            maxLines: 5,
            textCapitalization: TextCapitalization.sentences,
            decoration: const InputDecoration(
              hintText: 'Text',
              border: OutlineInputBorder(),
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

  Widget _bubble(BuildContext context, Entry e) {
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall!;
    final o = e.sent;
    final mine = o != null;
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
                SelectableText(e.text),
                const SizedBox(height: 4),
                Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(when(e.at), style: small),
                    if (mine && o.toward != null)
                      _where(o.toward!, small, scheme.onSurfaceVariant)
                    else if (!mine && e.received!.mailbox)
                      _mark(
                        Icons.markunread_mailbox,
                        'From your mailbox',
                        small,
                        scheme.onSurfaceVariant,
                      ),
                    if (!mine)
                      e.received!.acked
                          // One plain check: the person knows they have
                          // it, so the two coloured ones would say nothing.
                          ? _mark(
                              Icons.check,
                              'Acknowledged',
                              small,
                              scheme.onSurfaceVariant,
                            )
                          : _acknowledge(e.received!, small)
                    else if (o.acked)
                      _mark(
                        Icons.done_all,
                        'Acknowledged',
                        small,
                        scheme.primary,
                      )
                    else
                      ..._state(o, small, scheme.onSurfaceVariant),
                  ],
                ),
                if (mine) _actions(o),
              ],
            ),
          ),
        ),
      ),
    );
  }

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

  /// A sent text's state short of acknowledged. Not yet with the node
  /// shows a clock rather than nothing, so a missing mark never means two
  /// things. A state that is a problem keeps its words, which say why.
  List<Widget> _state(Outgoing o, TextStyle small, Color color) =>
      switch (o.state) {
        OutState.queued => [_mark(Icons.schedule, 'Queued', small, color)],
        OutState.writing => [
          _mark(Icons.schedule, 'Offering to the node', small, color),
        ],
        OutState.taken => [
          _mark(Icons.check, 'Taken by the node', small, color),
        ],
        OutState.givenBack || OutState.refused => [
          const SizedBox(width: 4),
          Flexible(child: Text(stateLabel(app.session, o), style: small)),
        ],
      };

  /// A filled button, unlike the plain check it turns into, so "tap to
  /// acknowledge" cannot be mistaken for "acknowledged".
  ///
  /// One line of small text tall, so the line keeps its height with the
  /// button in it.
  Widget _acknowledge(Received r, TextStyle small) {
    final line = small.fontSize! * (small.height ?? 1);
    return Padding(
      padding: const EdgeInsets.only(left: 4),
      child: SizedBox.square(
        dimension: line,
        child: IconButton.filled(
          tooltip: 'Acknowledge',
          icon: const Icon(Icons.check),
          iconSize: small.fontSize,
          padding: EdgeInsets.zero,
          constraints: BoxConstraints.tight(Size.square(line)),
          style: IconButton.styleFrom(
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          onPressed: () async {
            try {
              await app.messenger.acknowledge(r);
            } on BadPayload catch (e) {
              if (mounted) say(context, 'Not sent: $e');
            }
          },
        ),
      ),
    );
  }

  Widget _actions(Outgoing o) {
    final s = app.session;
    final again = o.state == OutState.givenBack;
    final drop = o.state != OutState.writing && o.state != OutState.taken;
    if (!again && !drop) return const SizedBox.shrink();
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (again)
          TextButton(
            onPressed: s.waitLeftS(o) > 0 ? null : () => s.offerAgain(o),
            child: const Text('Send again'),
          ),
        if (drop)
          TextButton(onPressed: () => s.drop(o), child: const Text('Drop')),
      ],
    );
  }
}
