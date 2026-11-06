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
  void dispose() {
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
    Toward.card => 'Their card\'s address',
  };

  /// When they last wrote, and whether from their home.
  String _heard(Contact c) {
    final at = c.heardAt;
    if (at == null) return 'Has not written yet';
    final from = c.wroteFrom;
    final where = !c.card.home || from == null
        ? ''
        : from.toString() == c.card.address.toString()
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
                Text(
                  mine
                      ? '${when(e.at)}, ${stateLabel(app.session, o)}'
                            '${o.toward == null ? '' : ', ${o.toward!.label}'}'
                            '${o.acked ? ', acknowledged ✓' : ''}'
                      : '${when(e.at)}'
                            '${e.received!.mailbox ? ', from your mailbox' : ''}'
                            '${e.received!.acked ? ', you acknowledged it' : ''}',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                if (mine) _actions(o) else _acknowledge(e.received!),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _acknowledge(Received r) {
    if (r.acked) return const SizedBox.shrink();
    return TextButton(
      onPressed: () async {
        try {
          await app.messenger.acknowledge(r);
        } on BadPayload catch (e) {
          if (mounted) say(context, 'Not sent: $e');
        }
      },
      child: const Text('Acknowledge'),
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
