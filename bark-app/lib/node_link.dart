// What the app needs of a connected node: the service's reads, writes
// and notifications (spec "Phone link"). The Bluetooth link and the fake
// node both implement it.

import 'dart:typed_data';

import 'link.dart';

/// How long a node is away at its reboot into a new image: 55 s from the
/// reboot to the node back, measured on the bench on 2026-11-06 for an
/// image of 227 pages (roadmap, verification, "A swap"), which the loader
/// hashes and swaps in, about 47 s by the datasheets (`bark-fw/README.md`,
/// "Updates"), before the firmware boots.
const Duration swapAway = Duration(seconds: 55);

/// The node answered a write with this ATT error.
class AttException implements Exception {
  final int code;
  const AttException(this.code);
  @override
  String toString() => 'ATT error 0x${code.toRadixString(16)}';
}

/// The phone's bond no longer opens the node `node`: a phone paired with
/// it removed this one (spec "Phone link", The removed phone).
class NotOurs implements Exception {
  final String node;
  const NotOurs(this.node);
  @override
  String toString() => '${shownName(node)} no longer knows this phone';
}

/// The phone still holds a bond for the node `node` that the node no
/// longer has: its owner unpaired it, or another phone paired with it.
/// Android encrypts every link to it with that bond, which the node
/// refuses, so the phone cannot use it even as a guest until the bond is
/// forgotten (spec "Phone link", Public).
class StaleBond implements Exception {
  final String node;
  const StaleBond(this.node);
  @override
  String toString() => staleBondLine(node);
}

/// What the person reads of a [StaleBond] at `node`: what happened and the
/// way out.
String staleBondLine(String node) =>
    '${shownName(node)} rejects this phone\'s old pairing. Forget it in '
    'Bluetooth settings.';

/// Android kept its bond with the node `node` when asked to remove it
/// before a new pairing, which cannot run over it: Android would pair
/// with its old keys while the node runs a new pairing (bench,
/// 2026-11-10). No pairing was tried.
class BondKept implements Exception {
  final String node;
  const BondKept(this.node);
  @override
  String toString() =>
      'The phone kept its old pairing with ${shownName(node)}, so no new '
      'one was tried. Forget the node in Bluetooth settings, then try '
      'again.';
}

/// The link to the node `node` ended before the app had read it: the
/// node ended it, or went out of reach.
class LinkEnded implements Exception {
  final String node;
  const LinkEnded(this.node);
  @override
  String toString() =>
      'The link to ${shownName(node)} ended. Try again nearer to it';
}

/// The pairing did not complete: it was cancelled, the code typed was
/// not the one on the node's screen, or the node keeps as many phones as
/// it can and refused it (spec "Phone link", Pairing).
class NotPaired implements Exception {
  const NotPaired();
  @override
  String toString() =>
      'Not paired: cancelled, a wrong code, or the node is full (its '
      'screen says PHONES FULL)';
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

  Future<void> setClock(int utcS, int offsetMin);
  Future<NodeTime> readClock();

  /// The mailbox key, or none.
  Future<void> setHome(List<int>? key);
  Future<NodeHome> readHome();

  /// The next read of Neighbours starts at `slot`.
  Future<void> setNeighboursFrom(int slot);
  Future<NeighbourPage> readNeighbours();

  /// Where an update stands.
  Future<UpdateProgress> readUpdate();

  /// Begins, commits or reboots ([encodeBegin], [encodeCommit],
  /// [encodeReboot]); throws [AttException] 0x81 when the node refuses,
  /// and [readUpdate] says why.
  Future<void> writeUpdate(Uint8List value);

  /// A piece of the image ([encodePiece]), without a response: the node
  /// takes it only where the bytes it holds end.
  Future<void> writePiece(Uint8List value);

  /// The phone's recipient tag at this node, by which a public node hands
  /// it its messages, with the app's versions and the person's `name`,
  /// which a paired phone's record keeps ([encodeRecipient]). Throws
  /// [AttException] 0x82 when the node and the app do not serve each
  /// other, and the node ends the link.
  Future<void> setRecipient(int tag, String name);

  /// The paired phone's alone: whether the node serves phones that have
  /// not paired.
  Future<bool> readPublic();
  Future<void> setPublic(bool public);

  /// The paired phone's alone: the node forgets its bond and ends the
  /// link.
  Future<void> unpair();

  /// On a guest's link: Guest, after which [readGuest] gives the oldest
  /// item with the phone's tag.
  Future<void> setGuestAfter(int number);

  /// Inbox as a guest's link gives it, in the Guest form; a guest does
  /// not confirm (spec "Phone link", Public).
  Future<GuestItem?> readGuest();

  /// The paired phone's alone: subscribes to Ask, whose answers
  /// [answerNotified] gives.
  Future<void> subscribeAsk();

  /// Asks the neighbour `id` for its status. The node answers the write
  /// at once; the neighbour's answer, if one comes, is notified.
  Future<void> ask(List<int> id);

  /// The paired phones' alone: the next read of Phones lists the records
  /// from place `slot` on.
  Future<void> setPhonesFrom(int slot);
  Future<PhonesPage> readPhones();

  /// The node draws a fresh removal code on its screen.
  Future<void> askRemovalCode();

  /// Removes the `bond`, the `home` or both of record `number`, with the
  /// `code` the node's screen shows; throws [AttException] 0x81 for
  /// another code or one past [removalCodeTime], which the node then
  /// forgets, and 0x13 for a record or part the node does not hold.
  Future<void> removePhone(
    int number, {
    required bool bond,
    required bool home,
    required int code,
  });

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
