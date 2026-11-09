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
    final card = me.shownCard;
    final home = me.home;
    final mono = const TextStyle(fontFamily: 'monospace');
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
                onPressed: () => askMyName(context, app),
              ),
            ),
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('My identity'),
              subtitle: SelectableText(me.key, style: mono),
            ),
            if (card == null) ..._noName(context) else ..._card(card),
            const SizedBox(height: 16),
            Align(
              alignment: Alignment.centerLeft,
              child: FilledButton.icon(
                icon: const Icon(Icons.qr_code_scanner),
                label: const Text('Scan their card'),
                onPressed: () => _scan(context),
              ),
            ),
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
            if (card != null)
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
          ],
        ),
      ),
    );
  });

  /// In place of the card while the person has no name.
  List<Widget> _noName(BuildContext context) => [
    const Padding(
      padding: EdgeInsets.only(top: 8),
      child: Text(
        'Set your name first: your card carries it, so that whoever scans '
        'it knows who you are.',
      ),
    ),
    const SizedBox(height: 16),
    Align(
      alignment: Alignment.centerLeft,
      child: FilledButton.icon(
        icon: const Icon(Icons.edit),
        label: const Text('Set my name'),
        onPressed: () => askMyName(context, app),
      ),
    ),
  ];

  List<Widget> _card(PersonCard card) => [
    Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Text(
        card.temporary
            ? 'Temporary card: you have no home node, so it names the node '
                  'you use now, which keeps your mail only while you come '
                  'back to it. Set a node that stays where it is as your home '
                  'in Settings.'
            : 'It carries your home node\'s address, whose mailbox keeps '
                  'your mail.',
      ),
    ),
    const SizedBox(height: 16),
    Center(
      child: card.temporary
          ? TemporaryFrame(child: _qr(card.toText()))
          : _qr(card.toText()),
    ),
  ];

  Future<void> _scan(BuildContext context) async {
    final c = await scanCard(context);
    if (c != null && context.mounted) await openScanned(context, app, c);
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

/// The mark of a temporary card around its QR code (roadmap D162): a
/// dashed frame and an hourglass outside the code's quiet zone, so a
/// scanner still reads it, and the word under it.
class TemporaryFrame extends StatelessWidget {
  final Widget child;
  const TemporaryFrame({required this.child, super.key});

  /// Room between the code's white edge and the frame, and for the glyph.
  static const _margin = 14.0;
  static const _glyph = 24.0;

  @override
  Widget build(BuildContext context) {
    final color = Theme.of(context).colorScheme.onSurface;
    final ground = Theme.of(context).colorScheme.surface;
    return Semantics(
      label: 'Temporary card',
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Stack(
            clipBehavior: Clip.none,
            children: [
              CustomPaint(
                painter: _Dashed(color),
                child: Padding(
                  padding: const EdgeInsets.all(_margin),
                  child: child,
                ),
              ),
              Positioned(
                top: -_glyph / 2 + 2,
                right: -_glyph / 2 + 2,
                child: Container(
                  color: ground,
                  child: Icon(
                    Icons.hourglass_empty,
                    size: _glyph,
                    color: color,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            'TEMPORARY',
            style: Theme.of(context).textTheme.titleMedium
                ?.copyWith(letterSpacing: 4, fontWeight: FontWeight.bold),
          ),
        ],
      ),
    );
  }
}

class _Dashed extends CustomPainter {
  final Color color;
  _Dashed(this.color);

  static const _dash = 10.0, _gap = 6.0;

  @override
  void paint(Canvas canvas, Size size) {
    final p = Paint()
      ..color = color
      ..strokeWidth = 2
      ..style = PaintingStyle.stroke;
    final r = Offset.zero & size;
    final edges = [
      (r.topLeft, r.topRight),
      (r.topRight, r.bottomRight),
      (r.bottomRight, r.bottomLeft),
      (r.bottomLeft, r.topLeft),
    ];
    for (final (a, b) in edges) {
      final d = b - a;
      final len = d.distance;
      final u = d / len;
      for (var t = 0.0; t < len; t += _dash + _gap) {
        final e = t + _dash < len ? t + _dash : len;
        canvas.drawLine(a + u * t, a + u * e, p);
      }
    }
  }

  @override
  bool shouldRepaint(_Dashed old) => old.color != color;
}
