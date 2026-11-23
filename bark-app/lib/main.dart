// A messaging app over a bark node: conversations, contacts and cards,
// settings, and a debug screen with everything the link says.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:sodium/sodium.dart';

import 'app.dart';
import 'l10n.dart';
import 'ui/common.dart' show Seconds;
import 'ui/conversations_screen.dart';
import 'ui/thread_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  speak(languageOf(WidgetsBinding.instance.platformDispatcher.locales));
  final app = await AppModel.onPhone(await SodiumInit.init());
  await app.load();
  app.links.startSearch();
  runApp(BarkApp(app));
}

class BarkApp extends StatefulWidget {
  final AppModel app;
  const BarkApp(this.app, {super.key});

  @override
  State<BarkApp> createState() => _BarkAppState();
}

class _BarkAppState extends State<BarkApp> with WidgetsBindingObserver {
  final _navigator = GlobalKey<NavigatorState>();
  late final AppLifecycleListener _life;
  late final StreamSubscription<String> _taps;
  Locale _language = languageOf(
    WidgetsBinding.instance.platformDispatcher.locales,
  );

  AppModel get app => widget.app;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _life = AppLifecycleListener(
      onStateChange: (s) {
        final front = s == AppLifecycleState.resumed;
        app.alerts.front(front);
        Seconds.instance.front = front;
      },
      // The person may have allowed Bluetooth in Android's settings, or
      // forgotten a node there.
      onResume: () {
        unawaited(app.links.checkBluetooth());
        unawaited(app.links.recheckKept());
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

  /// The person changed the phone's languages while the app ran.
  @override
  void didChangeLocales(List<Locale>? locales) {
    final language = languageOf(locales ?? const []);
    if (language == _language) return;
    speak(language);
    setState(() => _language = language);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _taps.cancel();
    _life.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    // The screens read their words when built, so a change of language
    // builds them all again, from the first screen.
    key: ValueKey(_language),
    title: 'bark',
    locale: _language,
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    navigatorKey: _navigator,
    theme: ThemeData(colorSchemeSeed: Colors.brown),
    darkTheme: ThemeData(
      colorSchemeSeed: Colors.brown,
      brightness: Brightness.dark,
    ),
    home: ConversationsScreen(app),
  );
}
