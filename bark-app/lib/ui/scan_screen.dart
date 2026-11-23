// Scans another phone's card with the camera; pops with the card.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:qr_code_scanner_plus/qr_code_scanner_plus.dart';

import '../card.dart';
import '../l10n.dart';

/// The camera's card, or null when the person goes back without one.
Future<PersonCard?> scanCard(BuildContext context) => Navigator.of(context)
    .push<PersonCard>(MaterialPageRoute(builder: (_) => const ScanScreen()));

class ScanScreen extends StatefulWidget {
  const ScanScreen({super.key});

  @override
  State<ScanScreen> createState() => _ScanScreenState();
}

class _ScanScreenState extends State<ScanScreen> {
  final _view = GlobalKey(debugLabel: 'QR');
  StreamSubscription<Barcode>? _scans;
  String? _problem;
  bool _done = false;

  @override
  void dispose() {
    _scans?.cancel();
    super.dispose();
  }

  void _created(QRViewController c) {
    _scans = c.scannedDataStream.listen((b) {
      final code = b.code;
      if (_done || code == null) return;
      try {
        final card = PersonCard.parse(code);
        _done = true;
        c.pauseCamera();
        if (mounted) Navigator.of(context).pop(card);
      } on BadCard catch (e) {
        // The camera reads the same code many times a second.
        if (_problem != said(e)) setState(() => _problem = said(e));
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final side = MediaQuery.of(context).size.shortestSide * 0.7;
    return Scaffold(
      appBar: AppBar(title: Text(tr.scanTitle)),
      body: SafeArea(
        top: false,
        child: Column(
          children: [
            Expanded(
              child: QRView(
                key: _view,
                formatsAllowed: const [BarcodeFormat.qrcode],
                onQRViewCreated: _created,
                onPermissionSet: (_, granted) {
                  if (!granted) {
                    setState(() => _problem = tr.scanNoCamera);
                  }
                },
                overlay: QrScannerOverlayShape(
                  borderColor: Theme.of(context).colorScheme.primary,
                  cutOutSize: side,
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(16),
              child: Text(_problem ?? tr.scanPoint),
            ),
          ],
        ),
      ),
    );
  }
}
