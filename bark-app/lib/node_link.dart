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

/// The phone's bond no longer opens the node `node`: another phone has
/// paired with it since (spec "Phone link", the replaced phone).
class NotOurs implements Exception {
  final String node;
  const NotOurs(this.node);
  @override
  String toString() =>
      '${shownName(node)} is no longer yours: another phone has paired '
      'with it';
}

/// The pairing did not complete: it was cancelled, or the code typed was
/// not the one on the node's screen.
class NotPaired implements Exception {
  const NotPaired();
  @override
  String toString() =>
      'Pairing did not complete: it was cancelled, or the code typed was '
      'not the one on the node\'s screen';
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

  /// Link version 2 and later.
  Future<void> setClock(int utcS, int offsetMin);
  Future<NodeTime> readClock();

  /// Link version 3 and later: the mailbox key, or none.
  Future<void> setHome(List<int>? key);
  Future<NodeHome> readHome();

  /// Link version 4 and later: the next read of Neighbours starts at
  /// `slot`.
  Future<void> setNeighboursFrom(int slot);
  Future<NeighbourPage> readNeighbours();

  /// Link version 6 and later: where an update stands.
  Future<UpdateProgress> readUpdate();

  /// Begins, commits or reboots ([encodeBegin], [encodeCommit],
  /// [encodeReboot]); throws [AttException] 0x81 when the node refuses,
  /// and [readUpdate] says why.
  Future<void> writeUpdate(Uint8List value);

  /// A piece of the image ([encodePiece]), without a response: the node
  /// takes it only where the bytes it holds end.
  Future<void> writePiece(Uint8List value);

  /// Link version 7 and later: the phone's recipient tag at this node,
  /// by which a public node hands it its messages.
  Future<void> setRecipient(int tag);

  /// Link version 7 and later, the paired phone's alone: whether the node
  /// serves phones that have not paired.
  Future<bool> readPublic();
  Future<void> setPublic(bool public);

  /// Link version 12 and later, the paired phone's alone: the node
  /// forgets its bond and ends the link.
  Future<void> unpair();

  /// Link version 8 and later, on a guest's link: Guest, after which
  /// [readGuest] gives the oldest item with the phone's tag.
  Future<void> setGuestAfter(int number);

  /// Inbox as a guest's link gives it, in the Guest form; a guest does
  /// not confirm (spec "Phone link", Public).
  Future<GuestItem?> readGuest();

  /// Link version 9 and later, the paired phone's alone: subscribes to
  /// Ask, whose answers [answerNotified] gives.
  Future<void> subscribeAsk();

  /// Asks the neighbour `id` for its status. The node answers the write
  /// at once; the neighbour's answer, if one comes, is notified.
  Future<void> ask(List<int> id);

  /// Asks for the shortest connection interval, for a transfer; where
  /// the platform chooses the interval itself, nothing.
  Future<void> hurry();

  Stream<Status> get statusNotified;
  Stream<GivenBack?> get givenBackNotified;
  Stream<Answer?> get answerNotified;

  /// Completes when the connection ends, from either side.
  Future<void> get closed;
  Future<void> close();
}
