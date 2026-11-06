// The person's own card, for another phone to scan or paste.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../app.dart';
import '../card.dart';
import 'common.dart';

class CardScreen extends StatelessWidget {
  final AppModel app;
  const CardScreen(this.app, {super.key});

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final me = app.me;
    final card = me.card;
    return Scaffold(
      appBar: AppBar(title: const Text('My card')),
      body: SafeArea(
        top: false,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('My name'),
              subtitle: Text(me.name.isEmpty ? 'not set' : me.name),
              trailing: IconButton(
                tooltip: 'Change',
                icon: const Icon(Icons.edit),
                onPressed: () async {
                  final n = await askText(context, 'My name', me.name);
                  if (n != null) await me.setName(n);
                },
              ),
            ),
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('My identity'),
              subtitle: SelectableText(
                me.key,
                style: const TextStyle(fontFamily: 'monospace'),
              ),
            ),
            if (card == null)
              const Padding(
                padding: EdgeInsets.symmetric(vertical: 16),
                child: Text(
                  'No address yet. Connect to your node: its address is its '
                  'position and node ID, and it has a position once it has a '
                  'GPS fix or a map point. Set it as your home in Settings '
                  'if it stays where it is.',
                ),
              )
            else
              ..._card(context, card),
          ],
        ),
      ),
    );
  });

  List<Widget> _card(BuildContext context, PersonCard card) {
    final home = app.me.home != null;
    final connected = app.session.address != null;
    final mono = const TextStyle(fontFamily: 'monospace');
    return [
      Padding(
        padding: const EdgeInsets.only(top: 8),
        child: Text(
          home
              ? 'The address is your home node\'s.'
              : 'The address is your node\'s now; it changes when the node '
                    'moves. Set a home node in Settings for one that stays.',
        ),
      ),
      if (!home && !connected)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'The address as last read from your node; connect to bring '
            'it up to date.',
          ),
        ),
      const SizedBox(height: 16),
      Center(child: _qr(card.toText())),
      const SizedBox(height: 16),
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

  /// Dark on white whatever the theme, as scanners expect, with the
  /// quiet zone of four modules the QR code standard asks for.
  Widget _qr(String data) => LayoutBuilder(
    builder: (context, box) {
      final qr = QrCode.fromData(
        data: data,
        errorCorrectLevel: QrErrorCorrectLevel.M,
      );
      final side = box.maxWidth;
      const quiet = 4;
      return QrImageView.withQr(
        qr: qr,
        size: side,
        backgroundColor: Colors.white,
        padding: EdgeInsets.all(side * quiet / (qr.moduleCount + 2 * quiet)),
      );
    },
  );
}
