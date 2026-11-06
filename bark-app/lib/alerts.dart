// What the app tells the person outside its own window: that it holds
// the link to a node, and which texts came while they were not looking.

import 'contacts.dart';
import 'session.dart';

/// The phone's side of notifying, so that the decision in [Alerts] can be
/// tested on the host.
abstract interface class Notifier {
  /// Whether the person lets the app notify.
  Future<bool> allowed();

  /// Asks the person to let the app notify; whether they did.
  Future<bool> ask();

  /// The link to the node called `node` is up: the app keeps running in
  /// the background, under a notification that says so.
  Future<void> linkUp(String node);

  /// The link is down: the app may be suspended again.
  Future<void> linkDown();

  /// Shows the unread `texts` from the contact of identity `key`, called
  /// `name`, as one notification in place of the last.
  Future<void> texts(String key, String name, List<Received> texts);

  /// Removes the notification of the contact of identity `key`.
  Future<void> clear(String key);

  /// The identity of each contact whose notification the person tapped.
  Stream<String> get tapped;

  /// Whether Android exempts the app from battery optimisation; null where
  /// there is no such thing.
  Future<bool?> batteryExempt();

  /// Opens the system's dialog to exempt the app.
  Future<void> askBatteryExempt();
}

/// No phone around the app: nothing is shown and nothing is asked.
class NoNotifier implements Notifier {
  const NoNotifier();
  @override
  Future<bool> allowed() async => true;
  @override
  Future<bool> ask() async => true;
  @override
  Future<void> linkUp(String node) async {}
  @override
  Future<void> linkDown() async {}
  @override
  Future<void> texts(String key, String name, List<Received> texts) async {}
  @override
  Future<void> clear(String key) async {}
  @override
  Stream<String> get tapped => const Stream.empty();
  @override
  Future<bool?> batteryExempt() async => null;
  @override
  Future<void> askBatteryExempt() async {}
}

/// Decides what to notify: each new text, unless its thread is in front
/// of the person, collected per contact until they open the thread.
class Alerts {
  final Notifier notifier;
  final Contacts contacts;
  Alerts(this.notifier, this.contacts);

  /// The app is in front of the person.
  bool _front = true;

  /// The identity of the contact whose thread is open.
  String? _open;

  /// The texts each contact's notification shows.
  final Map<String, List<Received>> _unread = {};

  /// The node the link notification names.
  String? _node;

  /// The link is up to the node called `node`, or down for null.
  Future<void> link(String? node) async {
    if (node == _node) return;
    _node = node;
    await (node == null ? notifier.linkDown() : notifier.linkUp(node));
  }

  /// A message was kept: a text notifies, anything else does not.
  Future<void> kept(Received r) async {
    final from = r.from;
    if (from == null || r.body == null) return;
    if (_front && _open == from) return;
    final unread = _unread[from] ??= [];
    unread.add(r);
    await notifier.texts(
      from,
      contacts.byKey(from)?.name ?? 'Unnamed',
      List.unmodifiable(unread),
    );
  }

  /// The thread with `key` is open.
  Future<void> opened(String key) async {
    _open = key;
    await _seen();
  }

  /// The thread with `key` is closed.
  void closed(String key) {
    if (_open == key) _open = null;
  }

  /// The app came in front of the person, or left.
  Future<void> front(bool f) async {
    _front = f;
    await _seen();
  }

  Future<void> _seen() async {
    final k = _open;
    if (!_front || k == null) return;
    _unread.remove(k);
    // Also one shown before the app last started, which it no longer knows.
    await notifier.clear(k);
  }
}
