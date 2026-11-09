// What each row of a node's sheet on the map means, for a person who
// taps it: in one place so that the texts are reviewed together. Each is
// checked against the spec section named beside it.

/// The help for a row, by its label: two to four plain sentences.
const Map<String, String> factHelp = {
  // Spec "Nodes", "Sleepy nodes".
  'Role':
      'A relay has outside power, such as USB, and listens all the time, '
      'passing on texts for the nodes around it: the more relays, the '
      'farther texts reach. A sleepy node is on battery: to last, it '
      'sleeps and wakes only in short windows, and sends and receives '
      'through one relay near it, its anchor. A fixed relay that loses '
      'its power keeps relaying on battery until the battery runs low, '
      'and then turns sleepy.',
  // Spec "Nodes", "Position", Fixed and carried.
  'Moving':
      'The node tells this itself from its GPS: after a little over a '
      'minute of positions away from where it stood, it counts as '
      'carried, and after four hours in one place it stays put again. A '
      'carried node\'s place follows its GPS, and it keeps no new mail '
      'for others, as it may soon be gone. Setting its place in the '
      'app\'s Settings makes it stay put at once.',
  // Spec "Relay choice", "Sleepy nodes", "Addresses".
  'Anchor':
      'A sleepy node sends and receives through one relay near it, its '
      'anchor. The anchor keeps the sleepy node\'s mail and hands it over '
      'in the short window after each time it wakes, and the sleepy '
      'node\'s address is its anchor\'s place. A sleepy node takes the '
      'first relay it hears as its anchor; with none yet it holds and '
      'passes on texts itself, at a higher cost to the channel.',
  // Spec "Position", "Addresses".
  'Place':
      'Texts find a node by its place: an address is a position and the '
      'node\'s ID, and each relay passes a text on toward that position. '
      'The place comes from the node\'s GPS, unless a person set it in '
      'the app\'s Settings, which wins, as for a relay indoors with no '
      'view of the sky. A node with no place yet sends no heartbeat, so '
      'no text can find it until it has one.',
  // Spec "Nodes", "Sleepy nodes", "Neighbour status".
  'Power':
      'USB means outside power, on which a node can relay all the time. '
      'On battery the figure is read from the cell\'s voltage, so it is '
      'rough; on this node\'s own sheet it is the raw reading in '
      'millivolts. At about a fifth left the battery counts as low, and '
      'a relay on it turns sleepy to save the rest for its owner.',
  // Spec "Phone link", Neighbours; "Neighbour table".
  'Link':
      'How strongly this node hears the other one, in dBm, and how far '
      'that is above the radio noise here, in dB. The margin is what '
      'counts: below about 4 dB a full text is often lost when the '
      'signal fades, so the link is fragile and used only when no other '
      'is left; from about 7 dB it is good. It is measured one way: the '
      'other node may hear this one better or worse.',
  // Spec "Heartbeat", "Beats", "Neighbour table", Expiry.
  'Last heard':
      'Every node sends a short heartbeat now and then, saying who and '
      'where it is. A relay sends one every 5 to 15 seconds, each on the '
      'next of 13 channels, so this node hears it about once every two '
      'to three minutes; a sleepy node wakes to send one every two and a '
      'half to five minutes. A relay not heard for six and a half '
      'minutes, or a sleepy node for ten, leaves the list.',
  // The app's own reckoning from the two places.
  'Distance':
      'The straight line from this node\'s place to the other\'s, worked '
      'out by the app from the two positions, so it is shown only when '
      'both have a place. How far radio reaches depends on height and on '
      'what stands between: relays on rooftops hear each other at about '
      '3 km.',
  // Spec "Neighbour status".
  'Known':
      'What the node says of its own neighbours, in three counts. Heard: '
      'the nodes whose heartbeats it hears. Strong: of those, the ones '
      'heard well enough that a full text gets through a fade, so they '
      'likely hear it too. Exchanged: of those, the ones that have taken '
      'a text from it, which proves they hear it.',
  // Spec "Updates over the mesh".
  'Version':
      'The version of the firmware the node runs. A new version spreads '
      'by itself: a node given it, from a phone or over USB, passes it to '
      'the relays around it a piece at a time, leaving the channels to '
      'texts. A node takes only a newer version signed by the project, '
      'and while it gathers one this row says which and how many of its '
      'blocks it holds.',
  // Spec "Phone link", Neighbours, flag bit 7.
  'Exchanged':
      'This node has handed it a text and it took it. That proves the '
      'link works both ways: it hears this node, as well as being heard.',
  // Spec "Neighbour table", Demotion.
  'Demoted':
      'Its last three hand-overs from this node went unanswered, so it '
      'may no longer hear this node. Until it answers again, or for six '
      'and a half minutes, it is tried only after every other neighbour '
      'with a good enough link.',
  // Spec "Bans".
  'Banned':
      'It turned a text down, asking for a wait, or a hand-over to it '
      'failed. Nothing is sent to it until the ban ends; texts go round '
      'it meanwhile, and wait for it only where there is no other way.',
  // Spec "Phone link", Sending, Receiving.
  'Paired':
      'This phone is paired with this node: the node sends your texts '
      'into the network, and keeps the ones for you until the app reads '
      'them. Only the paired phone sees the node\'s neighbours on this '
      'map.',
};
