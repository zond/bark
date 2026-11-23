// What each row of a node's sheet on the map means, for a person who
// taps it: in one place so that the texts are reviewed together. Each is
// checked against the spec section named beside it.

import '../l10n.dart';
import '../link.dart';

/// Which fact a row of a node's sheet gives: its identity, from which its
/// label and its help are made in the language spoken.
enum FactKind {
  role,
  moving,
  anchor,
  place,
  power,
  link,
  lastHeard,
  distance,
  known,
  gpsSignal,
  version,
  exchanged,
  demoted,
  avoided,
  paired,
}

/// A row's label, in the language spoken.
String factLabel(FactKind k) => switch (k) {
  FactKind.role => tr.factRoleLabel,
  FactKind.moving => tr.factMovingLabel,
  FactKind.anchor => tr.factAnchorLabel,
  FactKind.place => tr.factPlaceLabel,
  FactKind.power => tr.factPowerLabel,
  FactKind.link => tr.factLinkLabel,
  FactKind.lastHeard => tr.factLastHeardLabel,
  FactKind.distance => tr.factDistanceLabel,
  FactKind.known => tr.factKnownLabel,
  FactKind.gpsSignal => tr.gpsSignal,
  FactKind.version => tr.factVersionLabel,
  FactKind.exchanged => tr.factExchangedLabel,
  FactKind.demoted => tr.factDemotedLabel,
  FactKind.avoided => tr.factAvoidedLabel,
  FactKind.paired => tr.factPairedLabel,
};

/// The help for a row: two to four plain sentences, in the language now
/// spoken.
String factHelp(FactKind k) => switch (k) {
  // Spec "Nodes", "Sleepy nodes", A forced role.
  FactKind.role => tr.factRole,
  // Spec "Nodes", "Position", Fixed and carried.
  FactKind.moving => tr.factMoving,
  // Spec "Relay choice", "Sleepy nodes", "Addresses".
  FactKind.anchor => tr.factAnchor,
  // Spec "Position", "Addresses".
  FactKind.place => tr.factPlace,
  // Spec "Nodes", "Sleepy nodes", "Neighbour status".
  FactKind.power => tr.factPower,
  // Spec "Phone link", Neighbours; "Neighbour table".
  FactKind.link => tr.factLink,
  // Spec "Heartbeat", "Beats", "Neighbour table", Expiry.
  FactKind.lastHeard => tr.factLastHeard,
  // The app's own reckoning from the two places.
  FactKind.distance => tr.factDistance,
  // Spec "Neighbour status".
  FactKind.known => tr.factKnown,
  // Spec "Neighbour status", "Position", Checks.
  FactKind.gpsSignal => tr.factGpsSignal(gpsFixFloorDbHz),
  // Spec "Updates over the mesh".
  FactKind.version => tr.factVersion,
  // Spec "Phone link", Neighbours, flag bit 7.
  FactKind.exchanged => tr.factExchanged,
  // Spec "Neighbour table", Demotion.
  FactKind.demoted => tr.factDemoted,
  // Spec "Bans".
  FactKind.avoided => tr.factAvoided,
  // Spec "Phone link", Sending, Receiving.
  FactKind.paired => tr.factPaired,
};
