// A messaging app over a bark node: conversations, contacts and cards,
// settings, and a debug screen with everything the link says.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:sodium/sodium.dart';

import 'app.dart';
import 'ui/conversations_screen.dart';
import 'ui/thread_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final app = await AppModel.onPhone(await SodiumInit.init());
  await app.load();
  app.startSearch();
  runApp(BarkApp(app));
}

class BarkApp extends StatefulWidget {
  final AppModel app;
  const BarkApp(this.app, {super.key});

  @override
  State<BarkApp> createState() => _BarkAppState();
}

class _BarkAppState extends State<BarkApp> {
  final _navigator = GlobalKey<NavigatorState>();
  late final AppLifecycleListener _life;
  late final StreamSubscription<String> _taps;

  AppModel get app => widget.app;

  @override
  void initState() {
    super.initState();
    _life = AppLifecycleListener(
      onStateChange: (s) => app.alerts.front(s == AppLifecycleState.resumed),
      // The person may have allowed Bluetooth in Android's settings, or
      // forgotten a node there.
      onResume: () {
        unawaited(app.checkBluetooth());
        unawaited(app.recheckKept());
      },
    );
    _taps = app.notifier.tapped.listen(_openThread);
  }

  /// Opens the thread of a tapped notification over the first screen.
  void _openThread(String key) {
    final n = _navigator.currentState;
    if (n == null) {
      // The tap that launched the app comes before the first screen.
      WidgetsBinding.instance.addPostFrameCallback((_) => _openThread(key));
      return;
    }
    n.popUntil((r) => r.isFirst);
    n.push(MaterialPageRoute(builder: (_) => ThreadScreen(app, key)));
  }

  @override
  void dispose() {
    _taps.cancel();
    _life.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'bark',
    navigatorKey: _navigator,
    theme: ThemeData(colorSchemeSeed: Colors.brown),
    darkTheme: ThemeData(
      colorSchemeSeed: Colors.brown,
      brightness: Brightness.dark,
    ),
    home: ConversationsScreen(app),
  );
}
