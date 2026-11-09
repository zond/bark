// Notifying on Android. With the screen off the link to the node runs in
// this Dart session, as in the foreground: the app's own foreground
// service (LinkService.kt) keeps the process running under its
// notification, and the engine outlives the window (Engine.kt), so
// closing the app's window leaves the session running too. A service that
// read the inbox itself would be a second implementation of the link.

import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

import 'alerts.dart';

class PhoneNotifier implements Notifier {
  final _plugin = AndroidFlutterLocalNotificationsPlugin();

  // Single-subscription, so that a tap that launched the app waits for the
  // screens to listen.
  final _taps = StreamController<String>();

  /// The battery exemption, which no plugin used here offers
  /// (MainActivity.kt).
  static const _power = MethodChannel('org.bark.bark_app/power');

  /// The browser, for a link to download (MainActivity.kt).
  static const _browser = MethodChannel('org.bark.bark_app/browser');

  /// The share sheet, which no plugin used here offers (MainActivity.kt).
  static const _share = MethodChannel('org.bark.bark_app/share');

  /// The link's foreground service and its notification (LinkService.kt).
  static const _link = MethodChannel('org.bark.bark_app/link');

  /// The texts' notification ID; the link's, 1, is LinkService.kt's. Each
  /// contact's texts are told apart by a tag, their identity.
  static const _textsId = 2;

  static const _icon = 'ic_notification';

  /// A window of the app was closed: the next one may have been opened by a
  /// tap on a notification, which Android tells only as how it was opened.
  bool _newWindow = false;

  PhoneNotifier._();

  static Future<PhoneNotifier> start() async {
    final n = PhoneNotifier._();
    await n._plugin.initialize(
      settings: const AndroidInitializationSettings(_icon),
      onDidReceiveNotificationResponse: (r) => n._tap(r.payload),
    );
    AppLifecycleListener(
      onDetach: () => n._newWindow = true,
      onResume: () {
        if (!n._newWindow) return;
        n._newWindow = false;
        n._launched();
      },
    );
    // Started with the phone, or with a window not yet in front, the app
    // reads how the window was opened once it is.
    if (WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed) {
      await n._launched();
    } else {
      n._newWindow = true;
    }
    return n;
  }

  Future<void> _launched() async {
    final d = await _plugin.getNotificationAppLaunchDetails();
    if (d != null && d.didNotificationLaunchApp) {
      _tap(d.notificationResponse?.payload);
    }
  }

  void _tap(String? key) {
    // The link's notification carries no contact; it only opens the app.
    if (key != null && key.isNotEmpty) _taps.add(key);
  }

  @override
  Stream<String> get tapped => _taps.stream;

  @override
  Future<bool> allowed() async =>
      await _plugin.areNotificationsEnabled() ?? false;

  @override
  Future<bool> ask() async =>
      await _plugin.requestNotificationsPermission() ?? false;

  @override
  Future<void> hold(Held held) => _link.invokeMethod<void>('hold', _args(held));

  @override
  Future<void> letGo() => _link.invokeMethod<void>('letGo');

  @override
  Future<void> atBoot(Held? held) =>
      _link.invokeMethod<void>('atBoot', held == null ? null : _args(held));

  static Map<String, String> _args(Held h) => {
    'title': h.title,
    'body': h.body,
  };

  @override
  Future<void> texts(
    String key,
    String name,
    List<Unread> texts, {
    bool quiet = false,
  }) {
    final last = texts.last;
    return _plugin.show(
      id: _textsId,
      title: name,
      body: last.text,
      payload: key,
      notificationDetails: AndroidNotificationDetails(
        'texts',
        'Texts',
        channelDescription: 'A text from a contact',
        importance: Importance.high,
        priority: Priority.high,
        category: AndroidNotificationCategory.message,
        icon: _icon,
        tag: key,
        number: texts.length,
        when: last.at.millisecondsSinceEpoch,
        onlyAlertOnce: quiet,
        styleInformation: MessagingStyleInformation(
          const Person(name: 'You'),
          messages: [
            for (final t in texts)
              Message(t.text, t.at, Person(name: name, key: key)),
          ],
        ),
      ),
    );
  }

  @override
  Future<void> clear(String key) => _plugin.cancel(id: _textsId, tag: key);

  @override
  Future<bool?> batteryExempt() => _power.invokeMethod<bool>('exempt');

  @override
  Future<void> askBatteryExempt() => _power.invokeMethod<void>('askExempt');

  @override
  Future<void> openLink(String url) => _browser.invokeMethod<void>('open', url);

  @override
  Future<void> share(String text, Uint8List png) =>
      _share.invokeMethod<void>('share', {'text': text, 'png': png});
}
