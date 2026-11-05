// What the app needs of a connected node: the service's reads, writes
// and notifications (spec "Phone link"). The Bluetooth link and the fake
// node both implement it.

import 'dart:typed_data';

import 'link.dart';

/// The node answered a write with this ATT error.
class AttException implements Exception {
  final int code;
  const AttException(this.code);
  @override
  String toString() => 'ATT error 0x${code.toRadixString(16)}';
}

abstract class NodeLink {
  Future<NodeInfo> readNode();

  /// Subscribes to Status and Given back. The subscriptions are not kept
  /// across connections, so this follows every connect.
  Future<void> subscribe();
  Future<Status> readStatus();
  Future<GivenBack?> readGivenBack();
  Future<InboxMessage?> readInbox();

  /// Throws [AttException] 0x13 if `id` is not the oldest message's.
  Future<void> confirm(int id);

  /// Returns once the node has the message; throws [AttException] 0x80
  /// when it takes none now.
  Future<void> send(Uint8List value);
  Future<void> setMapPoint(Position? p);

  Stream<Status> get statusNotified;
  Stream<GivenBack?> get givenBackNotified;

  /// Completes when the connection ends, from either side.
  Future<void> get closed;
  Future<void> close();
}
