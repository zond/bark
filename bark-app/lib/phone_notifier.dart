// Notifying on Android. With the screen off the link to the node runs in
// this Dart session, as in the foreground: a foreground service only keeps
// the process running under its notification, and the activity keeps its
// engine when it goes (MainActivity.kt), so closing the app's window
// leaves the session running too. A service that read the inbox itself
// would be a second implementation of the link.

import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

import 'alerts.dart';
import 'session.dart';

class PhoneNotifier implements Notifier {
  final _plugin = AndroidFlutterLocalNotificationsPlugin();

  // Single-subscription, so that a tap that launched the app waits for the
  // screens to listen.
  final _taps = StreamController<String>();

  /// The battery exemption, which no plugin used here offers
  /// (MainActivity.kt).
  static const _power = MethodChannel('org.bark.bark_app/power');

  /// IDs among the app's notifications; each contact's texts are told
  /// apart by a tag, their identity.
  static const _linkId = 1, _textsId = 2;

  /// The link's notification makes no sound: it is there as long as the
  /// link, and Android requires it of a foreground service.
  static const _linkChannel = AndroidNotificationDetails(
    'link',
    'Connection to the node',
    channelDescription: 'Shown while the app keeps the link to a node',
    importance: Importance.low,
    priority: Priority.low,
    ongoing: true,
    showWhen: false,
    icon: _icon,
  );

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
    await n._launched();
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
  Future<void> linkUp(String node) => _plugin.startForegroundService(
    id: _linkId,
    title: 'Connected to $node',
    body: 'Texts arrive while the screen is off',
    notificationDetails: _linkChannel,
    // Started again by Android after it killed the process, the service
    // would claim a link that the process took with it.
    startType: AndroidServiceStartType.startNotSticky,
    foregroundServiceTypes: {
      AndroidServiceForegroundType.foregroundServiceTypeConnectedDevice,
    },
  );

  @override
  Future<void> linkDown() => _plugin.stopForegroundService();

  @override
  Future<void> texts(String key, String name, List<Received> texts) {
    final last = texts.last;
    return _plugin.show(
      id: _textsId,
      title: name,
      body: last.body,
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
        styleInformation: MessagingStyleInformation(
          const Person(name: 'You'),
          messages: [
            for (final t in texts)
              Message(t.body ?? '', t.at, Person(name: name, key: key)),
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
}
