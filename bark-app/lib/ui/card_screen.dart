// The person's own card, for another phone to scan or paste, and theirs
// to scan back.

import 'dart:ui' as ui;

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
    final mono = const TextStyle(fontFamily: 'monospace');
    Widget copyable(String title, String value, String what) => ListTile(
      contentPadding: EdgeInsets.zero,
      title: Text(title),
      subtitle: SelectableText(value, style: mono),
      trailing: IconButton(
        tooltip: 'Copy $what',
        icon: const Icon(Icons.copy),
        onPressed: () {
          Clipboard.setData(ClipboardData(text: value));
          say(context, '${what[0].toUpperCase()}${what.substring(1)} copied');
        },
      ),
    );
    return Scaffold(
      appBar: AppBar(title: const Text('My card')),
      body: SafeArea(
        top: false,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            if (card == null) ..._noName(context) else ..._card(card),
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                if (card != null)
                  FilledButton.icon(
                    icon: const Icon(Icons.share),
                    label: const Text('Share'),
                    onPressed: () => _share(context, card),
                  ),
                OutlinedButton.icon(
                  icon: const Icon(Icons.qr_code_scanner),
                  label: const Text('Scan their card'),
                  onPressed: () => _scan(context),
                ),
              ],
            ),
            const SizedBox(height: 8),
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
            ExpansionTile(
              title: const Text('Details'),
              tilePadding: EdgeInsets.zero,
              childrenPadding: EdgeInsets.zero,
              children: [
                copyable('My identity', me.key, 'the identity'),
                if (card?.home case final h?)
                  copyable('Home address', h.toString(), 'the address'),
                if (card?.now case final n?)
                  copyable('The node I use now', n.toString(), 'the node'),
                if (card != null) copyable('Card', card.toText(), 'the card'),
              ],
            ),
          ],
        ),
      ),
    );
  });

  /// The card as text and its QR code as an image, through the phone's
  /// share sheet.
  Future<void> _share(BuildContext context, PersonCard card) async {
    try {
      final text = card.toText();
      await app.notifier.share(text, await qrPng(text));
    } catch (e) {
      if (context.mounted) say(context, 'Sharing failed: $e');
    }
  }

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
    Center(
      child: card.temporary
          ? TemporaryFrame(child: _qr(card.toText()))
          : _qr(card.toText()),
    ),
    const SizedBox(height: 16),
    Text(
      card.temporary
          ? 'Temporary card: you have no home node, so it names the node '
                'you use now, which keeps your mail only while you come '
                'back to it. Set a node that stays where it is as your home '
                'in Settings.'
          : 'It carries your home node\'s address, whose mailbox keeps '
                'your mail.',
    ),
  ];

  Future<void> _scan(BuildContext context) async {
    final c = await scanCard(context);
    if (c != null && context.mounted) await openScanned(context, app, c);
  }

  /// Dark on white whatever the theme, as scanners expect, with the
  /// quiet zone of four modules the QR code standard asks for ([quiet]).
  Widget _qr(String data) => LayoutBuilder(
    builder: (context, box) {
      final qr = QrCode.fromData(
        data: data,
        errorCorrectLevel: QrErrorCorrectLevel.M,
      );
      final side = box.maxWidth;
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

/// The quiet zone around a QR code, in modules, which the standard asks
/// for (ISO/IEC 18004, 4 modules).
const quiet = 4;

/// `data` as a QR code in a PNG, dark on white with its quiet zone, for
/// the share sheet; [moduleSide] pixels a module, enough for a scanner
/// reading it off another screen.
Future<Uint8List> qrPng(String data, {int moduleSide = 8}) async {
  final qr = QrCode.fromData(
    data: data,
    errorCorrectLevel: QrErrorCorrectLevel.M,
  );
  final modules = qr.moduleCount + 2 * quiet;
  final side = (modules * moduleSide).toDouble();
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder);
  canvas.drawRect(
    Offset.zero & Size.square(side),
    Paint()..color = Colors.white,
  );
  canvas.translate(
    quiet * moduleSide.toDouble(),
    quiet * moduleSide.toDouble(),
  );
  QrPainter.withQr(
    qr: qr,
    gapless: true,
  ).paint(canvas, Size.square(qr.moduleCount * moduleSide.toDouble()));
  final image = await recorder.endRecording().toImage(
    side.toInt(),
    side.toInt(),
  );
  final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
  return bytes!.buffer.asUint8List();
}
