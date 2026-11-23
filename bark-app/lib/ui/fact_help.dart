// What each row of a node's sheet on the map means, for a person who
// taps it: in one place so that the texts are reviewed together. Each is
// checked against the spec section named beside it.

import '../l10n.dart';
import '../link.dart';

/// The help for a row, by its label as the English names it: two to four
/// plain sentences, in the language now spoken.
Map<String, String> get factHelp => {
  // Spec "Nodes", "Sleepy nodes", A forced role.
  'Role': tr.factRole,
  // Spec "Nodes", "Position", Fixed and carried.
  'Moving': tr.factMoving,
  // Spec "Relay choice", "Sleepy nodes", "Addresses".
  'Anchor': tr.factAnchor,
  // Spec "Position", "Addresses".
  'Place': tr.factPlace,
  // Spec "Nodes", "Sleepy nodes", "Neighbour status".
  'Power': tr.factPower,
  // Spec "Phone link", Neighbours; "Neighbour table".
  'Link': tr.factLink,
  // Spec "Heartbeat", "Beats", "Neighbour table", Expiry.
  'Last heard': tr.factLastHeard,
  // The app's own reckoning from the two places.
  'Distance': tr.factDistance,
  // Spec "Neighbour status".
  'Known': tr.factKnown,
  // Spec "Neighbour status", "Position", Checks.
  'GPS signal': tr.factGpsSignal(gpsFixFloorDbHz),
  // Spec "Updates over the mesh".
  'Version': tr.factVersion,
  // Spec "Phone link", Neighbours, flag bit 7.
  'Exchanged': tr.factExchanged,
  // Spec "Neighbour table", Demotion.
  'Demoted': tr.factDemoted,
  // Spec "Bans".
  'Avoided': tr.factAvoided,
  // Spec "Phone link", Sending, Receiving.
  'Paired': tr.factPaired,
};
