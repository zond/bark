// A messaging app over a bark node: conversations, contacts and cards,
// settings, and a debug screen with everything the link says.

import 'package:flutter/material.dart';

import 'app.dart';
import 'ui/conversations_screen.dart';

void main() {
  final app = AppModel.onPhone()..load();
  runApp(BarkApp(app));
}

class BarkApp extends StatelessWidget {
  final AppModel app;
  const BarkApp(this.app, {super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'bark',
    theme: ThemeData(colorSchemeSeed: Colors.brown),
    darkTheme: ThemeData(
      colorSchemeSeed: Colors.brown,
      brightness: Brightness.dark,
    ),
    home: ConversationsScreen(app),
  );
}
