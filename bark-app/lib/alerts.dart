// What the app tells the person outside its own window: that it holds
// the link to a node or looks for it, and which texts came while they
// were not looking.

import 'contacts.dart';
import 'conversations.dart';
import 'session.dart';

/// The phone's side of notifying, so that the decision in [Alerts] can be
/// tested on the host.
abstract interface class Notifier {
  /// Whether the person lets the app notify.
  Future<bool> allowed();

  /// Asks the person to let the app notify; whether they did.
  Future<bool> ask();

  /// The app keeps running in the background, under a notification of
  /// `held`'s title and body; a later call changes only the
  /// notification.
  Future<void> hold(Held held);

  /// The app need not run in the background: it may be suspended again.
  Future<void> letGo();

  /// The app starts with the phone, without a window, under a
  /// notification of `held` until it runs; for null it does not.
  Future<void> atBoot(Held? held);

  /// Shows the unread `texts` from the contact of identity `key`, called
  /// `name`, as one notification in place of the last; `quiet` changes
  /// what one still showing shows, without sounding again.
  Future<void> texts(
    String key,
    String name,
    List<Unread> texts, {
    bool quiet = false,
  });

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

/// A text a notification shows: as far as it has come, and when its first
/// part came.
typedef Unread = ({String text, DateTime at});

/// What the notification of an app kept running says: what it holds the
/// link for.
typedef Held = ({String title, String body});

/// No phone around the app: nothing is shown and nothing is asked.
class NoNotifier implements Notifier {
  const NoNotifier();
  @override
  Future<bool> allowed() async => true;
  @override
  Future<bool> ask() async => true;
  @override
  Future<void> hold(Held held) async {}
  @override
  Future<void> letGo() async {}
  @override
  Future<void> atBoot(Held? held) async {}
  @override
  Future<void> texts(
    String key,
    String name,
    List<Unread> texts, {
    bool quiet = false,
  }) async {}
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
/// of the person, collected per contact until they open the thread. A
/// text in parts notifies once, at its first part; a later one changes
/// what the notification shows.
class Alerts {
  final Notifier notifier;
  final Contacts contacts;
  Alerts(this.notifier, this.contacts);

  /// The app is in front of the person.
  bool _front = true;

  /// The identity of the contact whose thread is open.
  String? _open;

  /// The texts each contact's notification shows, each by the parts of
  /// it that have come.
  final Map<String, List<List<Received?>>> _unread = {};

  /// The texts in parts that have notified, by sender and first `sent`.
  final Set<String> _told = {};

  /// What the notification of the app kept running says, once told.
  Held? _held;
  bool _heldTold = false;

  /// Keeps the app running under `held`, or lets it go for null. The
  /// first call is passed on even for null: the app may have been started
  /// with the phone (`atBoot`) for a node it no longer looks for.
  Future<void> hold(Held? held) async {
    if (_heldTold && held == _held) return;
    _heldTold = true;
    _held = held;
    await (held == null ? notifier.letGo() : notifier.hold(held));
  }

  /// What the app starts with the phone under, once told.
  Held? _boot;
  bool _bootTold = false;

  /// Starts the app with the phone under `held`, or not for null.
  Future<void> atBoot(Held? held) async {
    if (_bootTold && held == _boot) return;
    _bootTold = true;
    _boot = held;
    await notifier.atBoot(held);
  }

  /// A message was kept: a text notifies, anything else does not.
  /// `parts` are those of its text that have come, by number.
  Future<void> kept(Received r, List<Received?> parts) async {
    final from = r.from;
    if (from == null || r.body == null) return;
    if (_front && _open == from) return;
    final unread = _unread[from] ??= [];
    final i = unread.indexWhere(
      (u) => u.nonNulls.any((x) => identical(x, r) || x.sameText(r)),
    );
    if (i < 0) {
      unread.add(parts);
    } else {
      unread[i] = parts;
    }
    final quiet = r.of > 1 && !_told.add('$from/${r.first}');
    await notifier.texts(
      from,
      contacts.byKey(from)?.name ?? 'Unnamed',
      List.unmodifiable([
        for (final u in unread)
          (text: Entry.received(u).text, at: Entry.received(u).at),
      ]),
      quiet: quiet,
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
