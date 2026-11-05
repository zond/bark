// The person's own card, for another phone to scan or paste.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app.dart';
import '../card.dart';
import 'common.dart';

class CardScreen extends StatelessWidget {
  final AppModel app;
  const CardScreen(this.app, {super.key});

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final cs = app.contacts;
    final a = cs.myAddress;
    return Scaffold(
      appBar: AppBar(title: const Text('My card')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('My name'),
            subtitle: Text(cs.myName.isEmpty ? 'not set' : cs.myName),
            trailing: IconButton(
              tooltip: 'Change',
              icon: const Icon(Icons.edit),
              onPressed: () async {
                final n = await askText(context, 'My name', cs.myName);
                if (n != null) await cs.setMyName(n);
              },
            ),
          ),
          if (a == null)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 16),
              child: Text(
                'No address yet. Connect to your node: its address is its '
                'position and node ID, and it has a position once it has a '
                'GPS fix or a map point.',
              ),
            )
          else
            ..._card(context, PersonCard(a, cs.myName)),
        ],
      ),
    );
  });

  List<Widget> _card(BuildContext context, PersonCard card) {
    final connected = app.session.address != null;
    final mono = const TextStyle(fontFamily: 'monospace');
    return [
      if (!connected)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'The address as last read from your node; connect to bring '
            'it up to date.',
          ),
        ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Address'),
        subtitle: SelectableText(card.address.toString(), style: mono),
        trailing: IconButton(
          tooltip: 'Copy the address',
          icon: const Icon(Icons.copy),
          onPressed: () {
            Clipboard.setData(ClipboardData(text: card.address.toString()));
            say(context, 'Address copied');
          },
        ),
      ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Card'),
        subtitle: SelectableText(card.toText(), style: mono),
        trailing: IconButton(
          tooltip: 'Copy the card',
          icon: const Icon(Icons.copy),
          onPressed: () {
            Clipboard.setData(ClipboardData(text: card.toText()));
            say(context, 'Card copied');
          },
        ),
      ),
    ];
  }
}
