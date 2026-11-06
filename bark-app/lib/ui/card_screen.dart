// The person's own card, for another phone to scan or paste, and theirs
// to scan back.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../app.dart';
import '../card.dart';
import 'common.dart';
import 'contacts_screen.dart';
import 'scan_screen.dart';

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
            ..._card(card),
            const SizedBox(height: 16),
            Align(
              alignment: Alignment.centerLeft,
              child: FilledButton.icon(
                icon: const Icon(Icons.qr_code_scanner),
                label: const Text('Scan their card'),
                onPressed: () => _scan(context),
              ),
            ),
            ..._copy(context, card),
          ],
        ),
      ),
    );
  });

  List<Widget> _card(PersonCard card) => [
    Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Text(
        card.home != null
            ? 'It carries your home node\'s address, whose mailbox keeps '
                  'your mail.'
            : 'No home node: people can answer you only after you write to '
                  'them. Set a node that stays where it is as your home in '
                  'Settings.',
      ),
    ),
    const SizedBox(height: 16),
    Center(child: _qr(card.toText())),
  ];

  List<Widget> _copy(BuildContext context, PersonCard card) {
    final mono = const TextStyle(fontFamily: 'monospace');
    final home = card.home;
    return [
      const SizedBox(height: 8),
      if (home != null)
        ListTile(
          contentPadding: EdgeInsets.zero,
          title: const Text('Home address'),
          subtitle: SelectableText(home.toString(), style: mono),
          trailing: IconButton(
            tooltip: 'Copy the address',
            icon: const Icon(Icons.copy),
            onPressed: () {
              Clipboard.setData(ClipboardData(text: home.toString()));
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

  Future<void> _scan(BuildContext context) async {
    final c = await scanCard(context);
    if (c == null || !context.mounted) return;
    await Navigator.of(
      context,
    ).push(MaterialPageRoute(builder: (_) => AddContactScreen(app, card: c)));
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
