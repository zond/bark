# bark-app

A plain Android messaging app over a bark node: it talks to the node
over Bluetooth LE with every part of the phone link, and seals what it
sends end to end (`docs/protocol.md`, "Phone link", "Mailbox";
`docs/phone-link-2026-11-05.md`, `docs/app-layer-2026-11-05.md`). It
is a prototype for the bench, not the app people will carry.

## Words

- **Node ID**: the radio's 6-byte number, shown on the node's display.
- **Address**: a node's position and node ID, 13 bytes, written as 26
  hex digits. It is what a message is sent to.
- **Identity**: the person's public key. The app makes it at first
  start from 12 words the person writes down, and can restore it from
  them on another phone. Texts are sealed to an identity.
- **Card**: what two people exchange so that each can reach the other:
  an identity, the name its owner chose, their home node's address
  when the person has set one, and, unless it is the home, the
  address of the node their phone is linked to, or was last, where to
  reach them now, with when it was last linked to it (roadmap D162). Without a home the card is temporary: the node on it keeps
  their mail only while they come back to it.
- **Contact**: a card with the name this phone gives it, or else the
  card's own, known by its identity. A card from an older app may have
  no name: its contact is listed as "Unnamed" with the first eight
  characters of its identity until a card with a name comes or the
  person renames it.
- **Home node**: a node on USB that stays where it is, set in Settings;
  it holds the person's mailbox key, and the person fetches their mail
  from it, when they ask, to the node they are connected to.

## Screens

- **Conversations**, the first screen. At the first start the app asks
  for Bluetooth at once, after one sentence on why: it finds and talks
  to a node over Bluetooth. Until the person has a name, a node (found
  by the search, or paired) and a first contact, a **Get started**
  card lists the three steps, each ticked when done and opening where
  it is done; the card goes once all are. One row a conversation,
  newest first, with the last text and, for a sent one, its state. A
  line at
  the top says which node the phone is on, in one of four ways:
  "Connected to your home, ID …"; "Connected to ID …, paired. Home at
  ID …" on another node the phone is paired with; "Connected to ID … as
  a guest. Home at ID …" during errands at a public node and between
  them; and "Not connected. Looking for ID …; texts wait on the phone",
  naming the home, another paired node or "a public node", or "Not
  connected. Bluetooth is off: turn it on to reach …". Without a home
  the home part is left out. With the search stopped it says "Not
  connected. Texts wait on the phone". Why the phone is on that node,
  or on none, is not in the line: its info button opens a few sentences
  that say, such as that none of the paired nodes has been in reach for
  30 s or that they are heard too weakly, or that the phone is paired
  with no node near the person.
  Settings' Node section shows the same line and button. Tapping the
  line opens Settings. Once there is a contact and
  until the backup words are written down, a card says "If you lose
  this phone, these words are the only way to keep your contacts able
  to reach you"; a tap opens Settings scrolled to the backup, which
  shows tinted for a moment and is announced. While the demo is on, a
  card says "Demo: nothing leaves this phone".
  A row swiped aside either way archives its conversation, with
  **Undo**; a line at the bottom then counts the archived ones, "2
  archived", and leads to Archived.
  The pen button starts a conversation from the contacts. With a home
  node, the inbox button in the bar, **Fetch my mail**, sends a fetch to
  it, which the app does only when asked, and says "Fetching your
  mail". The button is then off, an hourglass that says it waits for
  the home's answer and how long at most (32 minutes, the spec's
  bounds on the way there and back), until a reply comes, "No new mail"
  if the home has nothing new, or the node gives the fetch back. With
  no node connected the fetch waits, the button saying "Fetch waits for
  a node", and goes from the next node the phone links to, addressed to
  it. It sends nothing, and says why, when the connected node is the
  home, whose own inbox already holds the mail.
  When the person's card has changed and has not gone to their
  contacts, a line says why, "Your card changed: your home node has a
  new address" or "…: you have no home now", with **Send my card**,
  which sends it to every contact, one message each; the line then
  goes. The app sends no card by itself (roadmap D126).
- **A thread.** The texts sent to and received from one identity, in
  order, and a field at the bottom to write the next. Each text's small
  line has its time, then where it went, then its state, as icons the
  text's size whose tooltips give the words: a house, to their home; a
  pin, to where they were; an hourglass, to the node on a temporary
  card; a mailbox, from your mailbox. A sent text's states: a clock,
  waiting for a node, on this phone; two arrows, being handed to the node; a
  check, sent into the network, whose tooltip says the network does not
  say when it arrives, as it sends no such news (spec "Addresses"). A
  text the network gave back shows an hourglass and "The
  network is busy, sending again in m:ss", and the app sends it again
  by itself once the wait is over, also after a restart; until then it
  can be cancelled. A refused text says why in words. A tap
  anywhere on a sent text's bubble, its words too, opens a sheet that
  explains the states and their reaction, and a tap anywhere on a
  received one a sheet that explains the marks a received text can
  show: your reaction, a first message, from your mailbox, parts not
  yet here, the time. Above the marks, under This
  message, each sheet gives what the app keeps that the bubble does not
  say: for a sent text the node it went to and that node's place, its
  parts, how it was sealed and its message IDs; for a received one when
  the sender wrote it, by their phone's clock, its parts here, how it
  was sealed and its message IDs. The top line says
  when the contact last wrote, and, when their card names a home,
  whether from it. A long press on a received text opens its
  reactions, six emoji in a row of 48 dp buttons: thumbs up, heart,
  laugh, surprise, sad and thanks, each named in its tooltip, under a
  line that says the sender sees the one picked under their text and
  that another replaces it; and **Copy text**, as the long press no
  longer selects the words. The one picked goes at once, shows under
  the received text, and shows under the sender's copy when it arrives;
  a second pick replaces it there and goes again. Nothing answers by
  itself (roadmap D126, D185). A sent text's long press still selects
  its words. An acknowledgement from an app before reactions shows as a
  thumbs up.
  A text longer than one message holds, 156 bytes in a box without an
  address, goes as parts of up to 155 bytes, 16 at most (roadmap D137):
  once the text in the field needs more than one, a line under it says
  how many, "2 parts", counted in the bytes that will go, one a letter
  for Cyrillic. Send splits it between two characters, after a space
  near the end of each part, and queues the parts in order. The sent
  text is one bubble with the parts' state on its small line: the usual
  icons while every part is in the same state, else words such as
  "2 of 3 sent". A
  received text in parts is one bubble with the parts that have come,
  in order, and "part 2 of 3 not yet here" in place of one that has
  not, until all are in. A reaction is on the whole text, whichever
  parts have come.
  A contact made from a first message that came, with no sealed reply
  yet, is marked not verified, in its thread and its row; one from a
  card the person scanned or pasted is not, as the card came from its
  holder. Once it is verified, or for a scanned one, a text
  that came in a first message has a mark of its own, as anyone could
  have made it. A contact whose card is from an app before D162, with
  no home and no node, and who has not written yet, cannot be written
  to: a line says so where the field would be. The menu renames the
  contact,
  sends them your card, archives the conversation or the contact (both
  leave the thread, with **Undo**), or removes them, after asking; a removed
  contact's thread has only **Archive conversation**. The bar has **Fetch my mail**
  too, as on the first screen. Texts wait on the phone while no node is
  connected.
- **Contacts**, the head button beside the QR button. Each row says
  "Has a home node", or for a temporary card an hourglass and
  "temporary: at the node they used 20 min ago"; the mark goes
  when a card with a home comes. With none, it says how to add one.
  Adding one: scan their card with the camera, and the new contact's thread opens at once, so you see where
  it went and can write; or paste the card and save it, with a name of
  your choosing if you like. An address alone is not enough: a text is
  sealed to the identity on the card. Adding an archived contact brings
  it back. A row swiped aside archives the contact and its
  conversation, with **Undo**, and a line at the bottom counts the
  archived contacts.
- **Archived**, from the menu or the line at the bottom of either list.
  The archived conversations, then the archived contacts, each with an
  unarchive button. Unarchiving either brings back both: a person whose
  conversation is back is one you talk to again. A conversation opens
  read-only, with no field, reactions or menu, and
  **Unarchive** at the bottom. An archived contact is still known: its
  card and keys are kept, its messages open, and **Send my card** still
  goes to it. A new text from an archived conversation brings it, and
  its contact, back to the first screen, with the usual notification;
  a reaction or a card does not.
- **My card**, the QR button. It opens with the card as a QR code for
  another phone to scan, then **Share**, which hands the card as text
  and the code as an image to Android's share sheet, **Scan their
  card** to add theirs as in Contacts, and your name. A **Details**
  fold holds your identity, the home's address, the node's, and the
  card as text, each to copy. Without a home the card is temporary: a dashed frame around
  the code, an hourglass in its corner outside the code's quiet zone
  and the word TEMPORARY under it, and a line that says so. Without a
  name it shows no card: it says "Set your
  name first", with **Set my name**, as whoever scans a card needs to
  know whose it is.
- **Settings**, from the menu. The node to connect to (scan, then tap
  one; each says its signal as strong, at −70 dBm or above, fair, or
  weak, below −85 dBm, a choice, with the number in small print; a
  scan that has found nothing after about 10 s, or was stopped, says
  "No node found nearby" and to switch a node on or come closer; a
  public node says so, and a tap on one the phone is not paired with
  uses it as a guest, its **Pair** pairs with it (A guest's home,
  below)), how the connected node fares, and under
  "Paired with" the nodes the phone is paired with, the home first, each
  marked as the home, connected, not in reach while the search looks,
  "Not looking. Tap to connect" once the person has stopped the search,
  or no longer yours, with **Forget** (Unpairing, below). A tap on
  another paired node's row connects to it as its owner: at once when
  the app heard it in the last 30 s, the span the search takes to call a
  node out of reach, else after a scan of one round, saying it was not
  seen if it is not in reach. Below the list comes **Disconnect** while
  connected, or **Stop looking** while the search runs, apart from the
  rows so that Disconnect, which only ends the link, is not taken for
  Unpair. On a paired node of link version 7 or later the
  switch **Public: serve phones without pairing**, on unless the owner
  turns it off (Public nodes, below); on one of link version 13 or later
  the node's phones (Phones at the node, below); and from link version
  12 **Unpair**, on the connected node's row before link version 13 and
  on this phone's own row of the node's phones from it: a dialog names the node and
  says that it forgets this phone, shows `UNPAIRED` and is public again,
  that anyone at the node can pair with it by the code on its screen,
  that texts and contacts stay on the phone, which forgets its own
  pairing with the node too, and, with other paired nodes, that the
  phone stays paired with them; on **Unpair** the node forgets its bond
  and ends the link, the app forgets that node and looks for it no
  more, and then has Android remove its bond with the node (Unpairing,
  below). Then the home node: make
  the connected node your home if it is not, or none; the dialog says
  so, and on **Go ahead** your new card goes to every contact; on a
  public node the phone uses as a guest the same button pairs first,
  and its dialog says so (A guest's home, below); **Send
  my card**, as on the first screen; clear the mailbox key of a node
  that holds one and is not your home. A node without a place says
  "This node has no place yet", with **Use this phone's location**,
  which gives it the phone's position as its place at once and says the
  coordinates it set, as the same button under **Node's place** does; a
  sleepy or
  carried node cannot be made home, and the button is off with "A home
  must be a node that stays on power at a fixed place" (spec
  "Mailbox"). Then **Updates** (Newer releases, below), whether the app is
  exempt from battery optimisation (next section), your name, your 12
  words (show them, or restore an identity from them, after a warning
  that contacts holding the current card cannot reach the restored
  identity), and **Node's place**: set from typed degrees or the
  phone's location, or cleared. At the bottom, **Try without a node
  (demo)**, a node in the app, and **Debug**.
- **Nearby nodes**, from the menu: a map. The connected node and every neighbour it
  knows on an OpenStreetMap street map, a line from the node to each
  coloured by the link's quality: green good, amber fair, red fragile,
  by the margin of the link's averaged RSSI over the node's noise floor
  (spec "Phone link", Neighbours), dashed while the node avoids it,
  demoted or banned. A legend says the bands in dB and the icons: a
  relay is a tower; a sleepy node a battery; a carried one a walker; each in its link's colour;
  the node itself is a target. Tapping one shows the contact's name if
  the node is a contact's home or the node they last used, its ID, and
  then one row a fact in plain words, none of its screen's, an icon and
  a short label on the left and the value on the right, always in this
  order: Role (a tower, `relay: passes on others' texts`, or a moon,
  `sleepy: wakes in windows, speaks through its anchor`), Moving (a
  walker, `carried`, or a house, `stays put`), Anchor, for a sleepy
  node only (`anchored`, or `no anchor yet`; on this node's own sheet
  the anchor's ID, as the link names no neighbour's anchor), Place (a
  satellite, `from GPS`, a pin, `set by a person`, with the
  coordinates under it, or a crossed-out pin, `no place yet`), Power (a
  plug on USB, a battery with its percent), Link (an antenna, `good`,
  `fair` or `fragile` by the margin of the link's RSSI over the noise
  floor, in the link's colour, and in its help the numbers, such as
  "This link: −71 dBm, +19 dB over the noise floor."), Last heard (a clock), Distance (a ruler), Known
  (a hub, the neighbours it knows, such as `3 heard · 3 strong · 0
  exchanged`), Version (a price tag, the Material set having no version
  icon, such as `V18 · getting V19 14/32` while it gathers an image)
  and Exchanged (a ticked box) if it has taken a message from this
  node; then this node's own view of it: Anchor, `this node's anchor`,
  for the relay this sleepy node sends through, Demoted or Banned. Each
  icon is named for a screen reader. A tap anywhere on a row opens a
  few plain sentences on what that fact is, how the node knows it and
  what it means for the person, all in `lib/ui/fact_help.dart`. Its power, neighbours and version
  come from its answer (below); before one, what it is comes from its
  heartbeats. This node's own sheet says the same of it from its
  status, its neighbours counted from the table read. The map reads Neighbours when it opens, at every
  status the node gives, and when the refresh button is pressed. When
  it opens and when refreshed, on a node of link version 9 or later, it
  also asks each neighbour that has not answered within the last 10 s,
  through Ask, what its own screen shows;
  a neighbour that answers has its role under its marker in its
  screen's words, and its details the rows its answer gives, the image
  it gathers from link version 10. An ask or its
  answer is lost now and then on a busy channel, so a neighbour that
  has not answered within 2 s is asked again, a round at a time, up to
  three asks while the map is open (spec "Neighbour status"). The
  node is drawn at its own position, not its address, so a sleepy node
  is not drawn on its anchor; a node without a place has its
  neighbours drawn with no lines. Tiles come from the network and are
  kept on the phone for a year, so an area seen once draws again
  without internet; without either, the markers and lines are drawn on
  a blank background and a line says why. A node before link version
  4 cannot tell its neighbours, and the screen says to update it.
- **Debug**, at the bottom of Settings. Your identity, card address, home node and
  mailbox state; Node, what Home reads, every Status field, the last
  Given back value, each sent and received message's kind, `sent`, ID
  and payload in hex, and a log of the link's events since the app
  started. Each log line also goes to the Android log, so on the bench
  `adb logcat -s flutter` shows them as, for example,
  `11-06 14:03:27.513  8812  8840 I flutter : bark 14:03:27.512 Disconnected`.

A node keeps up to eight phones, each its owner (spec "Phone link",
Pairing; roadmap D168), and a phone may be paired with many nodes, and
Android keeps a bond for each (roadmap D166). The phone pairs with a node when the person makes it their
home or presses **Pair** on its row in Settings (A guest's home, below),
or taps a private node: Settings says to type the six-digit code the node's screen shows, and Android
asks for it in its own dialog; Android then takes about five seconds
before it reports the bond, and Settings says "Paired, connecting…"
until the node is read. While a connection is being made no other
starts, and the connected node's row in the scan says "Connected" and
does nothing: a second connect to it would close the link the first
made. Tapping another node's row closes the link and connects to that
one. A connection the person asks for holds against the search: from
the close of the old link until the new one is up the search neither
scans nor connects, as a round started by the close would take the
home before the tap's own connect was made (roadmap D179). The bond is the phone's from then on and
every later connection is encrypted without a code. When a phone paired
with the node has removed this one (Phones at the node, below), or, on
a node before link version 13, which kept one phone, another phone has
paired in its place, the node holds no key for this one: it
ends the link as soon as the phone asks to encrypt with its old key,
which fails the MTU request (Android's status 133, the connection's 5);
a refused first read, or a bond Android has dropped after such a
failure, count the same. A link that never comes up is a plain failure
to connect. The app then says "This node is
no longer yours", with **Not now** and **Pair again**, which forgets the
phone's old bond, scans until it sees the node again (about ten seconds
at most, else it says the node was not seen), and pairs anew, at the
node. The scan is needed because the node's address is random static,
and Android, with neither a bond nor a scan result to tell it so,
connects to it as a public address and times out after 30 s; the
Bluetooth plugin has no connect that takes the address's type. The
nodes paired with, and for each whether it was lost, are kept with the
queue and listed in Settings; pairing with one more adds it and keeps
the others. A queue file from before kept one node, which is read as a
list of one. A pairing cancelled or with a wrong code says
so, and that a node that already keeps eight phones refuses one too, its
screen then saying `PHONES FULL`. Pairing is the Bluetooth plugin's (`UniversalBle.pair`, Android's
bond), which needs no code of the app's own on the Android side.

**Public nodes.** Every node is public unless its owner turns the
switch in Settings off: it also passes on the texts of phones that have
not paired with it, guests, as a shop's or a café's node would, with
nothing to set (spec "Phone link", Public). A public node advertises
another UUID in place of the service's, and a private one the
service's, so a scan tells them apart without connecting; the app takes
a node as public unless its advertisement says it is private. A guest
connects without pairing and touches only what a guest may: Node,
Status, Send, Inbox, Given back, Recipient and Guest. It does not set the
clock, read Home or Public, set the map point, read the neighbours or
update the firmware, and Settings and the map hide those: Android may
start a pairing by itself when the node refuses a read for want of the
bond. On every connection to a node of link version 7 or later, the
owner's or a guest's, the app first writes the person's recipient tag
at that node to Recipient. A public node keeps every message that is
not its owner's in its ring, with the tag it carries, whether or not
its guest is there, until the ring wraps; it keeps nothing for a guest
beyond its link.

**Unpairing.** The node forgets its bond when its owner unpairs, but
Android keeps its own, and encrypts every later link to the node with
it, a guest's too, which the node ends (spec "Phone link", Public). The
node answers the write and ends the link, and Android may report the
write as failed when the end overtakes the answer (its catch-all GATT
error 133), so a failed write after which the link ends within 32 s,
the longest supervision timeout, counts as done; only a write that
failed while the link stayed up says "Unpairing failed", and then the
app forgets nothing. The search waits meanwhile, as a connect with the
bond still held would find the node no longer the phone's. Once the
node has answered, the app has Android remove the bond, by the
Bluetooth plugin's `UniversalBle.unpair`, and waits up to 2 s for
Android to list it no more. Android hides the removal from apps
(`BluetoothDevice.removeBond`, which the plugin calls by reflection and
whose failure it does not report), so the bond's state is the only
answer. Where the bond stays, a dialog says the node is public again
but that the phone could not forget its own pairing, with **Bluetooth
settings**, which opens the phone's Bluetooth settings (the intent
`android.settings.BLUETOOTH_SETTINGS`, `MainActivity.kt`), where the
person forgets the node. Either way, if the node was the person's home,
the line adds that it stays their home, so texts to them still go to
it: the node keeps the mailbox key. The search then uses the node as a
guest; the texts, the contacts, the home and the card stay as they
were.

**Forget**, on the row of a node no longer the phone's, takes the node
from the paired nodes after a dialog that says it no longer knows this
phone, so it is no longer the person's, that this phone stops listing it and forgets its own pairing
with it, that pairing by the code on its screen makes it the person's
again, and, for the home, that it stays the home, as after Unpair.
Android names a bond by the device address, which the app learns only
by hearing the node in a scan, and does not keep across runs. Where it
has heard the node since it started, it has Android remove the bond as
on unpairing, and where Android keeps it, says so with **Bluetooth
settings**. Where it has not, a dialog says that the app removes the
bond once it hears the node (A pairing found again, below) and that the
person may forget the node in the phone's Bluetooth settings now, with
the same button.

**Phones at the node.** On an owner's link to a node of link version 13
or later, Settings shows "Phones at this node", read from Phones page by
page at each connection and after each removal (spec "Phone link",
Phones): a line that a node keeps up to eight phones, that anyone at it
can pair by the six-digit code it shows and that every phone paired with
it may change its settings, so a phone not known or gone is best
removed; on a full node, that it takes no more pairings until one is
removed; then "Phones paired with this node" and "Phones with a home
here", a record that holds both listed in both, marked "has a home here
too" and "paired too". Each row is the person's name as their app wrote
it to Recipient (Versions, below), or "A phone with no name" for an app
before link version 13, with "this phone" on the reader's own, and
"linked now" or the day last linked as "today", "yesterday", "N days
ago" or "never", for a node that had no time at the phone's links.
Another's row has **Remove**, for what its record holds. Its dialog says
what goes and what that person sees: a pairing removed, the node ends
their phone's next link and their app says the node is no longer
theirs, to pair again by the code on the node's screen; a home removed,
the node forgets their mailbox key, their app says so at their next
link there, and their fetches from elsewhere go unanswered; and that
the node shows a code to type within 30 s. **Show the code** asks the
node for one, and its screen shows `REMOVE` and six digits; the person
types them and **Remove** writes the removal. A wrong code, or one after
30 s, says "Not removed", that each code takes one try, and offers
**New code**; a record the node no longer holds reads the list again.
This phone's own paired row has **Unpair**; its own home is changed
under the home node.

**A pairing found again.** Android may list a bond with a node the
app does not count as paired, as after the app's data was cleared, on
a new install, after restoring the 12 words, or from an older app that
kept only the node paired with last; the node may still hold the
phone's. Each round in which no paired node is heard and no link is
up, the search looks for a node heard that Android lists such a bond
with, the strongest first, and connects to it as its owner, at once and
before any public node. A link over a bond is never a guest's: a tap
on such a node as a public one, or the search's errand there, takes
the same way. A first read that succeeds on the encrypted link shows
the node still holds the bond: it is kept among the paired nodes. With
no home set and the node placed and able to be a home, the first screen
offers **Make this node my home** in one line, which says when the new
card would go to the contacts, and **Not now**, which hides it until
the app is started again. A node that refuses the bond goes the
way of a stale bond, below.

A phone can hold such a stale bond in other ways too: unpaired by an
older app, or with a node that no longer knows this phone. A
guest's link to a node that fails before the app has read it, or whose
first read is refused, while Android lists a bond with the node, is
taken as the node refusing that bond. The app then has Android remove
the bond, as on unpairing, scans until it sees the node again and
connects once more. Only where Android keeps the bond does it show one
line on the first screen and in Settings, "ID … no longer knows this phone's old
pairing. Forget it in the phone's Bluetooth settings, then try again.",
with the same **Bluetooth settings** button, and a tap on the node in
the scan says the same in a dialog. The search passes the node over,
as every link to it would fail alike, until Android no longer lists the
bond: it checks at each round and when the app comes to the front, and
the line goes once the bond is gone.

A node Android lists a bond with is never paired anew: Android pairs
with its old keys while the node runs a new pairing with a new code, and
the key check fails whatever code is typed (bench, 2026-11-10, DHKey
check failed three times). So **Pair** on such a node connects over the
bond first, and a node that still holds it is the person's again,
without a code; only a node that refuses it has Android remove the bond,
as **Pair again** does, and then pairs with the code its screen shows,
saying the stale-bond line only where Android keeps the bond.

**A guest's errand.** A public node takes one guest's link at a time
beside its owner's and ends it after 30 s, so a guest does not stay. It
connects, writes its tag, and reads its mail: it writes to Guest the
number of the last item it took from that node, 0 the first time, and
reads Inbox, which on a guest's link gives the oldest item after it
with the guest's tag, its number, message ID and payload, or nothing.
It keeps the item, writes its number, and reads again, until nothing
comes. Reading takes nothing from the ring. The app keeps the last
number for each node, by node ID, with the queue, so after a restart,
or a week away, it reads only what came since, as long as the ring
holds it; a node whose ring has restarted reads a number above its
newest as 0, so the app takes whatever number comes. Then it sends what
waits while Ready comes, one message each
time the node takes one, and leaves. The phones on a public node take
turns at the one message it holds, and the turn is Ready: the node sets
it for one phone at a time, so the app's queue waits for Ready as on
any node. A phone that connects while another guest has the link is
closed as it writes its tag; the next round tries again.

A guest's link the person makes by a tap on a public node in Settings
is not an errand: the app does not leave when the errand would, and
the search does not run beside it, so it lasts until the node ends it
after its 30 s, or the person goes to another node. The line says so,
"Connected to ID … as a guest, until the node ends a guest's link after
30 s", and its help that the node serves one guest at a time. Then the
search chooses again, the home first (roadmap D179).

The app goes on an errand at a public node when it has something to
send, at the next round, and otherwise once a minute (`guestCheck`): a
minute is how long the spec takes a person on foot to stay within a
relay's reach (Where a text goes), and a sender's app sends a reply to
where the person wrote from for that long, so a reply that came is read
within about as long as it was aimed there. It is four of the search's
rounds, so it costs no scan more within Android's five in 30 s; and an
errand of a few seconds a minute takes about a twelfth of the node's
one guest link, so a few phones in reach seldom meet there. Mail waits
in the ring meanwhile, so the interval delays it and loses none.

**A guest's home.** Only the paired phone gives a node the mailbox key
(spec "Phone link", Pairing), so a home is a node the phone has paired
with. While the phone uses a public node as a guest and has no home,
and the node has a place and can be a home, the first screen offers
**Make this node my home** as it does for a pairing found again
(above), its line saying what a home gives, that texts to the person
wait there and their card gets a fixed address, and that the phone
pairs with the node first, by the code its screen shows. Settings has
the same button under the home node, its dialog saying the same and
that a node keeps up to eight phones, so those paired with it before
keep theirs. The link
is down between errands, so the app keeps the node's address, whether
it can be a home and, from link version 13, Node's `room` from the last
errand there. A node whose `room` was 0 keeps eight phones already and
would refuse the pairing: the first screen does not offer it, the home
button is off, and a line says that
the node takes no more pairings and that someone paired with it can
remove a phone in Settings, at the node. A pairing that fails, a
wrong code or one cancelled, says why, and the phone goes on using the
node as a guest.

Whoever keeps a public node, or wants a standing link to it without
making it their home, presses **Pair** on its row in Settings' scan,
shown on each public node the phone is not paired with. Its dialog says
what pairing gives, the node's settings, its place and its updates on
this phone and a link the phone keeps while the node is in reach unless
the home is too, that a node keeps up to eight phones, and that the home
is set apart, under the home node. **Pair** connects and pairs at once
by the code on the node's screen, with no guest's errand first, so it
works while the home is in reach; a pairing that fails says why, and the
search then chooses again. A node whose `room` was 0 at the last errand
says it takes no more pairings instead (roadmap D179).

While it is not connected, the app looks for the nodes it is paired
with and connects to the first of them it hears, in this order: the
home, when the phone is paired with it; else the paired node heard
strongest, by the best RSSI each node had over the round. When it is
paired with none that is still its own, or they are out of reach, it
goes on its errands at the public node heard strongest (spec "Phone link",
Which node a phone uses). The order holds among the nodes heard
at −85 dBm or above, where Settings stops saying "weak"
(`FoundNode.usable`): a home or paired node heard below it is passed
over for a node further down the order heard above it, a public one at
once; with none heard above it, the app takes the node heard strongest,
whatever its kind (`chooseNode`). The paired nodes are out of reach
once two rounds in a row, 30 s, have reached none of them, heard or
connected (`ownGone`): one round with the screen off may miss a node in
reach. A paired node heard well always comes first, and the app moves
to it in the first round that hears it, so at the edge of its reach,
heard every other round, the phone stays with it. On a paired node that
is not the home the phone is its owner, with the standing link and the
owner's inbox, as on the home. There, while the phone is paired with
the home and the search made the link, the app scans for the home once
a minute (`lookAround`, the errand's `guestCheck`) and moves to it when
it hears it at −85 dBm or above and nothing on the link would be cut
(`settled`): no Send written and not yet answered, no inbox message
read and not yet confirmed, no fetch awaiting its answer
(`Session.inFlight`), no update sending, being checked or rebooting
(`Updater.underWay`); otherwise it tries at the next minute. The link
to the paired node closes, and the line changes to the home's. It never
moves between two paired nodes, and on the home, or on a node the
person tapped, it does not scan. A fetch sent from a public node brings
the mail back there, read
as a guest's. Each errand takes the
strongest public node of its round, so the phone moves to a nearer one
as it walks. The
app never joins as a guest the node that is no longer its own, nor one
that refused its old bond (Unpairing, above), while Android still holds
the old bond: it would encrypt the link with it, which the node ends.
Once Android holds none, the node serves the phone as a guest like any
public node.

A paired node is connected by the same path as a tap and without
a code; the search runs from the app's start, and after a link drops,
so that texts waiting go out as soon as a node is in reach. It runs
only once Android allows the app Bluetooth: a fresh install scans
nothing and shows no notification until the person presses **Scan** in
Settings, which asks for it. The app checks again when it comes to the
front and when Bluetooth is switched on or off, in case the person
allowed it in Android's settings. Bluetooth switched off ends the link
as a drop does: Android does not report that end itself. The first screen, and Settings in
the Node section, say "Not connected. Looking for ID …; texts wait on
the phone", naming the home when the phone is paired with it, else the
paired node connected last, or "a public node", or that Bluetooth is
off. A search round scans for four of a node's advertising intervals,
4.1 s, without a break, less once it hears the node no other would go
before, the home or a single paired node, and a round
starts every 15 s: Android refuses an app's
sixth scan started within 30 s, and two leave room for the person's own
Scan and Pair again. A phone's Bluetooth scanning without a break draws
in the order of 10 mA (an estimate, not measured here), so the search
costs about 3 mAh an hour, under a tenth of a percent of a phone's
battery, and the scan for the home from a paired node, once a minute,
about a quarter of that. The app stops looking, but for that scan, while
it is connected to a paired node, and while Bluetooth is off; a node of which it said "This node is no
longer yours" it looks for no more (the person decides with **Pair
again**), and with no other paired node it looks for public nodes
alone; and it stops after the
person presses **Disconnect** or **Stop looking** in Settings, until
they tap a node. That stop is kept with the
queue, so neither a restart of the app nor of the phone undoes it: the
person chose it, and the phone should not start looking behind their
back. A tap on a node, or a scan, ends a search round at once.

The app keeps its own queue and writes Send only while the node says
ready; on the ATT error 0x80, not ready, it waits for ready again.
Whenever Status counts messages in the inbox, it reads the oldest,
stores it, writes its message ID back so the node forgets it, and
repeats.

**Versions.** The app speaks link version 13 and serves nodes of link
version 1 and later, `oldestNode`: every value of version 1 is read and
written as then, and each characteristic or field a later version
brought is used only on a node of that version or later. The node says
in Node the oldest app version it serves, 12 for a node of version 13,
and a node before 13, which says none, is taken to serve its own
version alone. The app reads Node first and goes on only when each
side's version is at least the other's oldest (spec "Phone link",
Versions; roadmap D177); otherwise it closes the link and says which
side to update: "This app is older than this node's firmware serves:
update the app.", or "This node's firmware is older than this app
serves: update the node's firmware." To a node of version 13 or later
it then writes Recipient with the tag, its own link version and
`oldestNode`, and the person's name from their card on an owner's link,
cut to 32 bytes at a character's end; a guest writes no name, as the
node keeps nothing of a guest's. A node that finds the two do not serve
each other answers 0x82 and ends the link, and the app says the same.
A newer node that serves the app is used as one of its own version; the
**App** row in Settings' Updates says, without a notice, that the
node's firmware is newer and that an update of the app brings its new
features.
The first rotation's countdown runs on the phone,
since the node does not notify it. On every connection to a node of link
version 2 or later the app writes the phone's UTC time and offset to Clock,
silently, and the debug screen shows what the node then has.

On every connection to a node of link version 3 or later the app reads
Home. To the person's home, known by its node ID, it first writes the
key, silently, with a line in the log: the node may have been reflashed,
or the identity restored from its words, and a node that holds that key
already changes nothing. When a fixed, awake home's position is not
the one the person has for it, the app takes the new one, which changes
the card: such a node takes a new place at boot or when it turns fixed,
and the home is the node, not the spot. The app does not send the
changed card; the person sends it with **Send my card** (Screens).
A carried or sleepy home keeps the position the person has for it: a
carried node's position follows its GPS fix every second, and a sleepy
node's is its anchor's.

From link version 13 Home says whether the node is this phone's
person's home, `mine`, and the key goes to the home only where Home
says it is not theirs and the node's phones (Phones at the node, above)
list a home of their tag: one a phone of theirs left by unpairing,
which the write joins to this phone's record, or another phone of
theirs. Where none is listed, someone paired with the node may have
removed the home there, and the app tells the person rather than
overrule them: a notice says the home does not hold their mailbox
key and answers no fetch of theirs, and Settings shows **Make this node
my home** on the home, with the same line, to set it again.

The queue and the received texts, the contacts and what is archived,
and the person (seed, name, home, the last `sent`, the mailbox's
state) are kept in three
files in the app's private storage, so they survive a restart. The
seed is kept there in the clear: anyone who can read the app's storage
has the identity. Each file names the version of its format, 1 if it
names none; a file of another version, or one with any part that does
not read, is renamed to `<name>.old-<version>` or `<name>.old-bad`, with
`-2`, `-3` and on added if that name is taken, so no file set aside is
ever overwritten; the debug screen's log says so, and the app starts that
file empty (for the person, with a new identity). A contacts file from
before archiving names nothing archived and reads as such.

## Notifications and the background link

While a node is connected over Bluetooth, the app keeps running with the
screen off or its window closed, under a notification that says
which node it is on, in the first screen's words (Screens, above),
also during errands at a public node and between them; tapping it
opens the app. The link is the
same one as in the foreground: the app's Dart engine outlives its window
(`Engine.kt`), and an Android foreground service of the connected device
type (`LinkService.kt`) keeps the process running, so there is one
implementation of the link, in Dart. While the app looks for its paired
nodes (previous section) the service runs too, and its notification says
"Not connected. Looking for ID …; texts wait on the phone": without one
Android suspends or ends an app in the background, and the search with
it, and queued texts are to go out as soon as a node is in reach, not
when the person next opens the app. While it looks for a public node
alone, the notification names "a public node". Before Bluetooth is allowed there is no search, and no
notification.
The service goes on from connected to looking and back, changing only its
notification, as Android 12 and later restrict starting a service from
the background. It stops, and its notification goes, when the app neither
holds a link nor looks. With the screen off and the phone still, Android
may let the phone sleep through a round's pause, so a round can start
later than 15 s. The fake node runs inside the app and starts no service.

Each new text notifies, with the system's sound and vibration, under the
contact's name; a text in parts notifies once, at its first part, and
each later part changes what the notification shows, without a sound. Texts from one contact collect in one notification until
their thread is opened; tapping it opens the thread. Reactions and
cards do not notify, nor a text in a thread open in front of the person.

At the first connection the app says why it notifies, and Android asks
for the permission; the app does not ask again. Settings says whether the
app is exempt from battery optimisation and, if not, opens Android's
dialog to exempt it: without that, some phones stop the service after
some hours with the screen off.

**Starting with the phone.** When the phone has started, the app starts
the service by itself if it is paired with a node that is still its own
and the person has not stopped looking; a phone that held a link when it
was switched off thus looks for the node again and connects when it sees
it, with no one opening the app. The service starts the Dart engine
without a window and runs the same `main`, session and search; the
engine is kept, so a window opened later attaches to it and shows the
state as it stands, and closing the window leaves it running. The Dart
side tells the Android side, whenever it changes, whether to start at
boot and under what notification, which the Android side keeps in its
preferences: the rule needs the pairing and the stop, both known only to
Dart, and reading `bark.json` from Kotlin would make a second reader of
its format. The service is the app's own, not the notifications plugin's:
starting the plugin's from boot would mean writing its internal start
parameters by hand, and one small service that both shows the
notification and runs the engine is simpler. If Android ends the
process while the service runs, it may start the service again later,
which starts the engine as at boot.

The app starts on `BOOT_COMPLETED`, which comes after the person first
unlocks the phone, not on the earlier `LOCKED_BOOT_COMPLETED`: until then
the app's files, the identity among them, are encrypted, and moving them
to storage readable before unlocking would leave the identity less
protected. Android delivers the boot broadcast only to an app the person
has opened at least once since installing it, and not after **Force
stop** in Android's settings until it is opened again. The battery
exemption (above) matters for the service to run for hours, and some
makers' battery managers end it anyway, or keep the app from starting at
boot, until the person allows it in the maker's own settings.

## The fake node

A switch in Settings replaces Bluetooth by a node written in Dart
(`lib/fake_node.dart`) that answers as the spec says, with an invented
network around it: a first rotation of 195 s in which it hears three
relays; every third message it takes is given back with a wait of
16 bans, 26 s; every fourth write is answered not ready for one ban,
1.58 s. Behind it is one person, the **fake friend**, with an identity
of their own: they start with two first messages to you in the inbox,
with their card, and answer each text you send them with a sealed
"Echo: " of it 5 s later; a text in parts once every part has come,
split again, its last part first and the others 5 s after, so that
the missing ones show. Around it stand five neighbours, by slot as a
table holds them, with gaps: one of each link quality over a noise
floor of −118 dBm, a sleepy one on a map point, a demoted one and a
banned one, within about 700 m of its fix; a test may add more to
read the table across pages. Their card names a home, `bark-fa4e00000002`,
and they write from a node of their own away from it,
`bark-fa4e00000003`, which moves about 110 m north between their
messages. It runs on while the app is disconnected, as
a node does. It counts as paired with the phone; a test can have a
phone paired with it remove this one (`FakeNode.pairElsewhere`), and
the app then finds it no longer yours, as with a node. It speaks this
app's link version unless a test gives it another (`version`,
`oldest`), and answers Recipient as a node does, 0x82 and the link
ended where the two do not serve each other. Its records are this
phone's, made of its bond, its home, the tag and the name written to
Recipient, and those a test adds (`others`), which Phones lists as a
node does, page by page; it says `room`, refuses a pairing when full,
draws a removal code on `askRemovalCode` (`shownCode`), and takes a
removal as a node does: 0x13 for a record or part it does not hold,
0x81 for another code or one past 30 s, a code tried once. Public, as it starts, until
the switch or a test makes it private, it takes one guest's link beside
its owner's, closes a second guest's as it writes its tag, and ends a
guest's after 30 s; a guest's link answers 0x05, insufficient
authentication, to every value that needs the bond, and 0x13 to a write
to Inbox. It keeps a message in the owner's inbox when it carries the
owner's tag, or the owner has written none yet, and the rest in a ring
of 1024 by number, which a guest reads by its tag after the number it
writes to Guest; a test can make a node nobody has paired with
(`owned`), whose messages all go to the ring. Its turns are Ready: set for one phone at a time, in the
order of their links, passing on when the node takes that phone's
message, when its link ends, or after a ban unused; a Send out of turn
is answered 0x80, not ready. It takes an update as a node does, and its
new image hears no relay, unless a test says so
(`FakeNode.hearsAfterUpdate`), so that the warning of a node that hears
no one after its update (Updating a node) can be seen.

## Updating a node

A node out of reach, on a roof, is updated from the phone over the link
(spec "Phone link", Updates; roadmap D145). Settings, under
**Updates**, says the version the connected node runs on the **Node
firmware** row; its **Send to the node**, when the latest release has a newer
image, opens the update screen, which takes the image from **Latest
release**, the `bark-fw-<F>.img` of the latest release the app knows of
(Newer releases), signed by the release that built it. For the bench, Debug, under
**Bench**, has **Pick a firmware file**, which opens the same screen
with **Pick a file** beside it for an image
`bark-fw/scripts/image.py pack` made and `image.py sign` signed; the
node refuses an image no key of its own signed (`bark-fw/README.md`,
"Updates"). **Latest release** is offered only when that release's
image is newer than the node's; otherwise a line under the buttons
says why: no release known yet, or the node already runs the latest.
The app checks the file's header and hash before it offers it, and shows its
version, and says so when it is not newer than the node's: the node
would refuse it. **Send to the node** sends it with a progress bar, in
windows of seven pieces of 240 bytes, each window followed by a read of
where the node stands (`lib/updater.dart`); it asks Android for a short
connection interval first, and an image of 400 KB takes about 20 s, up
to a minute where Android does not grant it. If the link is lost, the
screen says how far it got, and when the app connects again it goes on
from the bytes the node holds, not from the start. Once every byte is
there the app commits: the node checks the image and answers, and the
screen says it keeps it for its next boot, or why it refused, in words.
A refusal for a low battery keeps the image on the node, and
**Try again** asks the node to check it again once it is on USB. **Reboot into version
N** then restarts the node, which swaps the image in, about a minute.
The node answers and ends the link, and Android may report the write
failed when the end overtakes the answer; a link that ends after such a
failure is the reboot, not an error. The app looks for the node and
connects again by itself, and the screen says the node runs version N,
updated from version M just now, or so many minutes ago; the image,
its progress and its buttons go until another is picked. If the new
image did not start, the node swapped the old one back, and the screen
says the version it came back on. An image from the phone keeps itself
whatever the node hears, since the node may stand alone
(spec "Phone link", Updates). So if the node still hears no other node,
sleepy ones counted, 585.9 s after it came back, as long as an image
from the mesh would listen, the update screen and a bar at the top of
the conversations say that the node hears no one and that the new
firmware may have broken its radio, and how to bring it back: the release it ran before, `bark-fw-<M>.uf2`, copied to
it over USB after a double tap of its reset button, since the node
takes no older version from the phone; or a newer release, once there
is one. The bar's × hides it until the next update. A node from before
link version 11 says only the relays it knows, which the app counts
instead, so it may warn of one that hears only sleepy nodes. A node from
before link version 6 cannot be updated this way: Settings says to
update it by USB once (`bark-fw/README.md`, "Flash").

## Newer releases

At start, and once a day while it runs, the app asks GitHub's releases
API, without an account, for the repository's latest release `v<N>`
(`lib/releases.dart`; `README.md`, "Releases"), and downloads its
`release.json`, `{"release": N, "app": A, "firmware": F}`: the app's
version A and the node firmware's F that the release carries, each N
when the release built it or an earlier release's carried on unchanged.
A release without the file, v21 and older, carried one version, N, and
the app reads both as N. When the release carries an APK newer than the
app, a bar at the top of the conversations says "A newer app, version A,
is available"; a tap opens the APK's download in the browser, and
Android asks before it installs it over the app. When the connected node
runs a version below F, a bar says "Your node runs version M; version F
is available"; a tap opens **Update firmware**
with the release's image downloaded and chosen (Updating a node), and
for a node from before link version 6 the bar says to update it by USB
once. A bar's × hides it until a newer version, the node's for that
node alone. The app learns its own version from the release workflow,
which builds it with `--dart-define=BARK_VERSION=<N>` and names it A in
the manifests of that release and of those that carry it on; a build without
it is a development build, which Debug says, and which never asks for
a newer app. A node of version 0, a bench build flashed by USB, gets no
bar either: any signed image replaces it, and the bench updates it by
USB. Without a network nothing shows, and the app asks again at its
next start. The last answer, its time and the versions hidden are kept
in `release.json`, so the bars show at once at the next start while the
app asks again.

Settings' **Updates** section has **Check for updates**, which asks at
once by the same check, with a spinner while it runs, and then says in
one line how this app and the node stand against the latest release,
such as "App up to date (22); node firmware 21, up to date", "App up to
date (22); node firmware 21; version 23 is newer: send it under Node
firmware below", "A newer app, version 22, is available: get it under
App below; no node linked", or "Could not reach the release list"
without a network. Under it the **App** row gives the app's version and,
when the release is newer, a tap downloads it; the **Node firmware** row
gives the node's version and, when the release's image is newer, **Send
to the node**. A dismissed
bar does not hide a newer version here.

## The payload

What the app sends is sealed with libsodium (the `sodium` package,
which builds libsodium itself into the app), in the forms of spec
"Phone link": a first message to someone in a sealed box with your
card until a message of theirs has opened here, which shows they hold
your card, and from then on boxed to their identity, which names no
sender: the app tries its contacts' keys on each message that arrives.
A contact is verified once a boxed message of theirs opens, as only
they can make one. So whoever writes first introduces themselves, the
answer is boxed and verifies the one who answered, and the next message
back verifies the first; if both write before either reads, both
introduce themselves, and both verify on their next. A first message
from a verified contact, perhaps one a later box overtook, adds only
its text, marked. A
message names the node you write from while the contact may not know
it, carries
the time it was written as its `sent`, and holds a text or a part of
one, a reaction or a card. A fetch to your home node is sealed with a
key from your identity that only your phones and your home node have,
and the node's replies are opened with it: one for each item newer than
the last this phone has, or one that says nothing is new (spec
"Mailbox"). A payload
that opens for nobody is kept for the debug screen only. Each payload's
second byte, in the clear, is its recipient's tag at the node it goes
to, made from the recipient's identity and that node's ID, by which a
public node tells whose it is (Public nodes, above); a fetch carries
your tag at your home, and inside it your tag at the node the replies
are to come to, which the replies carry. No receiver checks it.

**Making a node the home.** The connected node must be of link version
3 or later. The app writes 1 and the mailbox key to Home, reads Home
back, and makes the node the home only if it then says it holds a key,
from link version 13 the person's (`mine`);
otherwise it says so and changes nothing. **No home node** while
connected to the home writes 0, and the node forgets the key. Away from
the home the key cannot be taken back: the app forgets the home and says
that the old home holds the key until the person connects to it and
presses **Clear its mailbox key** there, which writes 0. The same
happens to the old home when another node is made the home. Settings
shows that button on any node that holds the person's key and is not
their home, with a line saying it still holds their key, from when it
was their home; before link version 13, whose Home does not say whose a
key is, on any node that holds a key, with a line saying it is
someone's home, perhaps a former one of theirs. A node of version 13
that holds another person's key says "This node is someone else's
home", with no button: a phone writes and clears only its own person's
key, and another's goes only by a removal (Phones at the node).


## Where a text goes

A message names the node its sender writes from, while the other side
may not know it, and a card names the node its owner's phone uses, with
when it was last linked to it, so the app knows two addresses for a
contact: the home on their card, if they have one, and one place they
were last known at, from whichever of their messages and cards is
newest: a message counted from the time it was written, or from when
it arrived if that is earlier, a card from its link time. While that
place is under a minute old, a text goes there without asking, straight to
where they are, with no fetch from their mailbox: in a minute a person
on foot, at a brisk 1.5 m/s, moves about 90 m, within the ≈750 m a
relay hears a handheld in the street (`docs/protocol.md`, "Physical
layer"), while a driver has already left that reach, so no longer wait
makes the guess better. Older, the send
button asks, for that text only: "Where <name> was <age> ago", "Fast if
they are still near there", or "Their home mailbox", "Waits at their
home until they fetch it". Without a home it goes where they were, as
nothing else is; when that is old, the dialog says "Last seen there 2 h
ago: it keeps their mail only while they come back" and still sends
there. With a home and no place known yet, it goes to the home (roadmap
D132, D162). Reactions and
cards go without asking, to the home's mailbox when there is a choice.

## Plugins

Besides those for Bluetooth, location and storage, pinned packages
handle the QR code, the cryptography and the notifications, all under
licences compatible with the repository's GPL-3:

| package | does | licence |
|---|---|---|
| `sodium` 4.1.2+1 | libsodium 1.0.22, built from its source into the app, and its Dart binding | BSD-3-Clause; libsodium ISC |
| `crypto` 3.0.8 | SHA-256, for the backup words' checksum and a firmware image's hash | BSD-3-Clause |
| `file_selector` 1.1.0 | picks a firmware image file | BSD-3-Clause |
| `qr_flutter` 4.1.0, with `qr` | draws the code | BSD-3-Clause |
| `qr_code_scanner_plus` 2.3.2 | scans it with the camera | BSD-2-Clause; on Android it uses the ZXing libraries, Apache-2.0 |
| `flutter_local_notifications` 22.3.3 | the texts' notifications and the permission to notify | BSD-3-Clause |
| `flutter_map` 8.3.2, with `latlong2` 0.10.1 | the map, its OpenStreetMap tiles and their cache on the phone | BSD-3-Clause; `latlong2` Apache-2.0; the packages they bring MIT or BSD; the tiles' data © OpenStreetMap contributors, ODbL, which the map names on screen |

The common `mobile_scanner` is not used: on Android it decodes with
Google's ML Kit, which is not free software.

## Building

Flutter 3.47 or later, the Android SDK, and a full JDK 17 or 21 (with
`javac`).

```
cd bark-app
flutter test
flutter build apk --debug
```

The APK is `build/app/outputs/flutter-apk/app-debug.apk`; install it
with `adb install`. If the build says a Java toolchain "does not provide
the required capabilities: [JAVA_COMPILER]", Gradle found a runtime
without a compiler: set `JAVA_HOME` to a full JDK, for example
`JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64`.

`flutter build apk --release` signs with the key that
`android/key.properties` names, when that file exists:

```
storeFile=release.jks
storePassword=...
keyPassword=...
keyAlias=...
```

`storeFile` is relative to `android/`. Without the file a release build
is signed with the debug key, except in CI (`CI=true`), where it fails:
the runner's debug key is lost with the runner, and an APK signed with
it could not be updated by the next. Both files are kept out of the
repository by `android/.gitignore`; the release workflow writes them
from the repository's secrets (`README.md`, "Releases").

The tests check every value's bytes against the vectors of
`bark-link/src/tests.rs`, the message ID among them; check keys,
words, cards and every payload form against `bark-app-layer`'s
`vectors.json`, made by the Rust side; run the queue, the inbox, first
messages, reactions and an older app's acknowledgement, a reflected
message, a forged first message,
the header's written-from address kept apart from the card's, the
recipient tag on every payload, and the
mailbox's replies, and long texts sent as parts, put together in any
order and reacted to, against the fake node, which answers a fetch
as a home does; pass messages between two phones by hand, in every
order of who scans and who writes first, until both are verified,
and with a first message read after a box, delayed or forged; the
fetch button's wait, ended by a reply, a give-back
or its bound, and after a restart; give the fake node the
mailbox key, read it back, clear it, write it again at a connection to
the home, at every one before link version 13 and from it only where a
home of the person's tag is listed, tell the person where someone
removed it, and refuse a node that does not keep it; make the fake
public, join it as a guest that writes its tag and touches nothing
closed, read a guest's ring items by its tag after the number it keeps
for the node, across links and restarts, close a second guest, end a
guest's link after 30 s, and take turns as Ready;
choose the home among the paired nodes, else the strongest paired
node, else the strongest public node; go on a guest's errand (connect,
tag, read, send, leave) when something waits to be sent and otherwise
once a minute, try again after being closed for another guest, and move
to a paired node once it is heard; with paired nodes, go to a public
node only after two rounds without any, send through it as a guest,
name it on the card and on the first screen's line, come back to a
paired node in the round that hears it, and stay with a paired node
heard every other round; say each of the four lines, with the home and
without, and why in the line's help; keep every node paired with, each
lost apart, across restarts, and read a queue file of one; list them in
Settings, the home first, with Unpair on the one connected, connect to
one on a tap, after a scan when it was not heard, and say "Not looking.
Tap to connect" with the search stopped, Disconnect below the list;
use a node of a newer link version that still serves the app, as its
owner and as a guest, with the quiet line, and close one that does not,
by its Node or by its answer to Recipient, or one older than the app
serves, saying which side to update; write the name and the versions
to Recipient, and the tag alone to a node of version 12; list a node's
phones in two lists with their names and days, remove another's with
the code its screen shows, and offer a new code after a wrong one; say
that a full node takes no more pairings, and try none there; make a
guest's node the home by pairing first, offered on the first screen
and in Settings, and stay a guest after a wrong code; from the home,
take a tapped public node as a guest with no round between and keep it
until the node ends it, and pair with one by its Pair at once; take back among
the paired nodes a node Android holds a bond with that the app does not
count as paired and that still holds the phone's, at once, by the
search or a guest's tap, and never as a guest; have
Android remove an old bond a public node refuses and connect again,
and where Android keeps it, pass the node over, saying so once, until
the bond is forgotten; on unpairing, have Android forget its bond too,
also when the link ends under the write, say what to do where it keeps
it, and forget nothing when the write fails with the link up; read the fake
node's neighbours, in one page and across several, and not from a node
before link version 4; group texts into
conversations; split texts at boundaries as the Rust vectors do;
choose where a text goes; decide what notifies, and when the app starts
with the phone, against a notifier that writes down what it is asked;
and drive the screens: a text to the fake friend, its echo, a
reaction each way, the backup words, a contact from a pasted or a scanned
card, my card, the debug screen, the fetch button, the choice of where
a text goes, a long text's parts, the bars for a newer release against
a fake fetch, and the map, its markers, lines,
legend and a neighbour's details, over tiles that cannot be had. The Android side, the service and
the boot receiver, cannot run on the host; it is kept small and tried on
a phone (Trying it, With a node).

## Trying it

### With the fake node

1. Open the app. It says why it needs Bluetooth, and Android asks. The
   first screen says no conversations, with the **Get started** card.
   Tap the line "Not connected", at the bottom turn on **Try without a
   node (demo)**, and at the top press **Connect**. The first time, the
   app says why it notifies: **Continue**, and allow it. It shows
   `ID FA4E00000001`, connected, "Starting up, 3:14" counting down, then
   "Hears 3 nodes".
2. Go back. The first screen says "Demo: nothing leaves this phone". A
   notification of the fake friend's two messages has come, and with
   the fake friend a contact, the first screen asks you to write down
   your 12 backup words: tap it, and Settings opens at **Backup**,
   tinted for a moment. **Show my words**, write them down, **I wrote
   them down**.
   A conversation with **Fake friend** holds their two
   messages.
3. **Settings**, at the bottom, **Debug**: your identity and card address; link version 13,
   the node ID `fa4e00000001`, the clock from the phone with the
   phone's offset, Home "not a home", an address, and Status: ready,
   external power, placed, first rotation running with about 3:15 to go,
   the relays known and the neighbours heard rising from 0 to 3. The log shows the two inbox
   messages read and confirmed.
4. Open the conversation: it says not verified, and the top line "Last
   wrote just now, away from home". Write a text and send it. It shows
   as sent into the network, "to where they were"; 5 s later "Echo: " and
   your text appears, and the note goes: the echo was sealed by the
   fake friend.
5. Press and hold the echo, and pick the heart: it shows under the
   echo, and Debug lists a `reaction` sent. Press and hold it again
   and pick another: it takes the heart's place, and a second
   `reaction` is listed.
6. Send three more. The third is given back with a wait counting down
   from 0:26. When the wait is over, press **Send again**. This is the
   fourth write, which the fake answers not ready: the text stays
   queued for about two seconds, then it is taken.
7. Wait a minute after the last echo, then send a text: the send
   button asks "Where Fake friend was 1 min ago" or
   "Their home mailbox". Pick the mailbox: the text says "to their
   home". Its echo makes where they were fresh again, and the next text
   goes there without asking.
8. Write a text of more than 160 letters: a line under the field says
   "2 parts". Send it: one bubble with the whole text, its parts'
   state in words while they differ. When both are taken, 5 s later the
   echo's last part arrives with "part 1 of 2 not yet here" in its
   place, and 5 s after that the whole echo.
9. The QR button, **My card**: set a name. A QR code appears, marked
   temporary without a home; **Share** opens Android's share sheet with
   the card's text and the code's image, and **Details** your identity
   and the card as text, `BARK:` and capitals.
10. Menu, **Settings**, under **Node's place**: press **Use this
    phone's location** (allow location): a line says the coordinates
    set, Debug's address changes, and the
    next text to the fake friend names it. **Clear** brings back the
    fake's own fix.
11. **Settings**, under **Home node**: **Make this node my home**. The
    dialog says your card changes and goes to your contact. **Go ahead**: "This node is your home". Debug's Home reads "a home, its
    mailbox empty". Go back: the bar has the inbox button, **Fetch my
    mail**. Press it: it says you are connected to your home node, and
    sends nothing. Disconnect and press it: it says no node is
    connected.
12. Connect again. **Settings**, **No home node**, **Go ahead**: the
    node forgets the key, and Home reads "not a home". Make it your home
    again, disconnect, press **No home node**: the app says the old home
    still holds the key. Connect: Settings says the node holds a mailbox key; **Clear its
    mailbox key** clears it.
13. Menu, **Nearby nodes**: the fake node in Kyiv and its five neighbours,
    three green lines, one amber and one red; the red one and a green
    one dashed. Tap the red
    one: `ID FA4E00000103`, fragile, 3.0 dB over the floor, demoted.
    Within half a second most markers show their screens' roles, such
    as `FIXED RELAY`; the fake loses one ask in three, so one more
    shows about 2 s later, asked again, and the one that never answers
    is asked three times in all (Debug's log, `Ask:`). **Read again**
    within 10 s asks only that one; later, all of them.
    With the phone offline and the area never seen, a line says the
    street map is missing and the nodes are drawn on blank.
14. Disconnect, close the app, open it again: the conversations, the
    contacts, your name and your identity are still there.
15. **Settings**, under **Updates**: **Node firmware** says the fake runs
    version 1. Debug, **Bench**, **Pick a firmware file**, or with a release known
    **Send to the node**, then **Latest release** (or **Pick a file** with a signed
    image; the fake refuses one whose header names no key):
    the image's version and size. **Send to the node**: the bar fills at
    once, the fake has no radio to wait for; then "The node checked
    version N". **Reboot into version N**: the link closes, and the
    fake takes no connection for 55 s, as a node swapping; then
    **Connect** in Settings, and the screen says "ID … runs version
    N, updated from version 1 just now.", without the image or the bar.

### With a node

1. Switch the node on and read the twelve hex digits on its display.
2. In **Settings**, with the demo off, press **Scan** and allow
   Bluetooth. The node shows within about a second as `ID` and those
   digits, as its screen shows them. Tap it: the phone uses it as a
   guest, and Settings says so. Under the home node, **Make this node my
   home**, whose dialog says the phone pairs first, or **Pair with this
   node** below it. Android asks for a code and the node's screen shows
   `PAIR CODE` and six digits within a few seconds: type them. A wrong
   code says the pairing did not complete, and the phone goes on as a
   guest. Settings then says "Connected and paired." and how the node
   fares: "Starting up, m:ss" in its first rotation, "Hears N nodes",
   sleepy ones counted, or "Hears no other node yet: texts wait here";
   and
   the node's screen, `PHONE UNPAIRED` before, `PHONE PUBLIC`.
3. **Debug** shows link version 13, the node's clock (from its GPS if it
   has a fix, else from the phone, with the phone's offset) and, on a node
   fresh from boot, the
   first rotation counting down from 3:15 (a relay) or 6:30 (a sleepy
   node), the relays it knows and the neighbours it hears, sleepy ones
   too.
4. A node with a GPS fix or a map point shows "Placed: yes" and an
   address, and **Nearby nodes** draws it with the relays it hears, each line's
   colour its link's quality. Without one, Settings under **Home node**
   says "This node has no place yet" with **Use this phone's location**,
   or set a place under **Node's place**: Status changes to
   placed and map point. A text names the node's address only once it
   has one. With neighbours on link version 9 or later, each marker
   shows its screen's role within a second or, asked again, a few;
   the node's log says `ble: ask` for each ask, `status of` for each
   answer, and `StatusGivenUp` for an ask or answer given up on a busy
   channel, which tells it from one lost on the air.
5. With a second node and a second phone, make one phone's node its
   home (step 9 below; without one the card is temporary, and texts
   to it wait at the node it names), then each phone scans the other's card from **My card**,
   **Scan their card** (allow the camera). Send a text from the other
   phone to the one with the home: it shows as taken, and the other phone shows
   it in the sender's conversation within the time the network takes.
   Status's inbox count on the receiving node, in Debug, rises to 1
   and falls to 0 as the app reads it. Answer it: the answer is boxed,
   and the phone that wrote first shows the other as verified. Write
   again from that phone: the phone with the home shows it as verified
   too. Press and hold the answer and pick a reaction: it shows under
   the answer, and the other phone shows it under its text when it
   arrives.
6. Switch off the receiving phone's Bluetooth, send two texts to it,
   switch it back on and connect: both arrive, oldest first, and the
   inbox count goes back to 0.
7. With the receiving phone connected, its screen off for ten minutes,
   send it a text: it sounds and shows the text under the sender's
   name. A second text joins the same notification. Tapping it opens
   the thread, and the notification goes.
8. Given back and not ready happen only when the network refuses; they
   show as in step 6 with the fake node.
9. On a node on USB that stays where it is, **Settings**, **Make this
   node my home**: Debug's Home reads a home, and the node's log shows
   `ble: home set`. Texts to your card now go to it and wait in its
   mailbox. Connected to another node, **Fetch my mail** on the first
   screen says "Fetching your mail" and brings them there, or "No new
   mail"; the home's log shows `mailbox: fetch … taken` and a
   `mailbox: reply …` line for each reply.
10. Two phones on one node, of link version 13, its log open. With the
    first phone paired and its name set, pair the second with the same
    node, its name set too: it asks for the code, the node's screen shows
    a new one, and once typed the screen says `2 PHONES PUBLIC`; the
    first phone stays connected and stays paired. On each phone,
    **Settings**, "Phones at this node" lists both names under "Phones
    paired with this node", its own marked "This phone, linked now" and
    the other "Linked now". Make the node the first phone's home: the
    first phone's row shows under "Phones with a home here" too, on both
    phones once they read the list again (connect again). On the first
    phone, **Remove** on the second's row under "Phones paired with this
    node", **Show the code**: the node's screen shows `REMOVE` and six
    digits. Type a wrong code: "Not removed", and the log shows `ble:
    removal refused`; **New code**, type the one shown: "Removed", the
    log shows `ble: record … removed`, the second phone's link ends, and
    the screen says `PHONE PUBLIC`. The second phone, connecting again,
    says "This node is no longer yours", with **Pair again**, which takes
    the code on the screen. With eight phones paired, a ninth's pairing
    shows no code, the screen says `PHONES FULL`, and the ninth says the
    pairing did not complete, naming a full node among the reasons; as a
    guest there it says the node takes no more pairings. A phone with an
    app of link version 12 on the same node still works, its row "A phone
    with no name".
11. Starting with the phone: open the app once after installing it,
    exempt it from battery optimisation in Settings, and connect to the
    node. Restart the phone and unlock it, without opening the app.
    Within about half a minute of the unlock the notification says
    "Not connected. Looking for ID …; texts wait on the phone", then
    "Connected to …", and texts arrive.
    `adb logcat -s flutter` shows the session's log from the start,
    `Found bark-…, connecting` and `Connected, MTU 247` among it.
    Opening the app shows the link as it stands. After **Disconnect**, a
    restart of the phone starts nothing.
12. An update of the node's firmware to a new link version, with the
    phone paired and the app updated first: connect again without
    pairing. Debug shows the new version, and the inbox, the status and
    the map read as before. Once Android has subscribed to Service
    Changed, which the node's log shows as an `att write` to its
    descriptor, the log shows `ble: service changed: the phone saw
    version` with the old and the new version, then `ble: service
    changed confirmed`; the next connection logs neither.
13. An update from the phone (Updating a node), on a node of link
    version 6 or later with the loader, version N, on USB with its log open. On
    the host, from `bark-fw/`, build an image one version up, open for
    the bench, and copy it to the phone:
    `BARK_OPEN=1 BARK_IMAGE_VERSION=N+1 cargo build --release &&
    scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw
    target/vN+1.img`, then sign it with the bench key, which an open
    build holds: `scripts/image.py sign target/vN+1.img keys/bench.pem
    --key-id 255`. In **Settings**, **Updates**, **Node firmware** says
    version N. Debug, **Bench**, **Pick a firmware file**, the image:
    "Version N+1".
    **Send to the node**: the node logs `update: receiving version=N+1`
    and then nothing while the bar fills, about 20 s; half way, switch
    the phone's Bluetooth off and on: the node logs `update: link closed
    with … bytes of version N+1 held`, the screen says how far it got,
    and once the app has connected again the bar goes on from there, the
    app's log saying `Update: goes on from …`. Then `update: staged`,
    a few seconds of the hash, and `update: committed version=N+1`; the
    screen says the node keeps it. **Reboot into version N+1**: `update:
    reboot asked by the phone`, `ble: disconnected 0x16`, `reboot`; about
    a minute later the log's banner says `image version N+1` and
    `update: version N+1 confirmed`, the app connects again, and the
    screen says "ID … runs version N+1, updated from version N just
    now.", without the image or the bar. Picking the same file again
    shows it, and sending it is refused, as not newer; an image packed and not signed is
    refused at once, before any piece, as not signed by a key the node
    holds, and the node logs `update: begin refused=Signature(Unsigned)`.
14. Newer releases. While the repository has no release, or the phone is
    offline, the conversations show no bar; Debug says "Latest release:
    not known" and its log `Checking for a new release failed:` with
    the reason, `HTTP 404` when there is no release. Release `v1`
    shows nothing new either: no version but 0 is below 1, and version
    0 never asks. From release N of 2 or more, with the app built and
    installed from a checkout as
    `flutter build apk --debug --dart-define=BARK_VERSION=1` and a node
    flashed by USB from a build with `BARK_IMAGE_VERSION=1`: Debug says
    app version 1 and "Latest release: N, app A, firmware F, checked" a
    moment ago, A and F as the release's `release.json` names them; the
    bar "A newer app, version A, is available" opens the APK in the
    browser, and its × hides it, also after the app is closed and
    opened again. Connected to the node, the bar "Your node runs
    version 1; version F is available" opens **Update firmware** with
    "Version F" chosen, and its × hides it for that node; another node
    of version 1 still shows it. A node of version 0 shows none.
15. A public node, on link version 8 or later, with two phones. On the owner's
    phone, connected, **Settings**, **Public: serve phones without
    pairing** is on from the start, and the node's screen says `PHONE
    PUBLIC`; turned off, it says `PHONE PRIVATE`, and on again `PHONE
    PUBLIC`. On a second phone that has never paired with it, **Scan**
    lists the node as public with its signal; with no paired node of
    its own in reach, the second phone goes on an errand there within a
    round, without asking anything, its first screen saying "Connected
    to ID … as a guest", with an errand once a minute,
    and, with no home set, offering **Make this node my home**; **Not
    now** hides it until the app is started again. No pairing dialog
    shows, at the connect or later. Its log shows `Recipient: tag …`, a
    `Guest: item …` line for each item read, and `Disconnected`.
    A text between the two phones' contacts reaches each, and neither
    sees the other's; a text sent to the second phone while it is away
    from the node is read at its next errand there. Taken near its own
    paired node, the guest phone moves to it within a round. The owner's
    phone, its node switched off or carried away, says "Connected to ID …
    as a guest. Home at ID …" within about 30 s if another public node is
    in reach, its info button saying that none of its paired nodes has
    been in reach for 30 s; it sends a text through it, and says
    "Connected to your home, ID …" within a round of the node's return.
16. Unpairing, on link version 12 or later. On the owner's phone,
    connected, **Settings**, **Unpair** on the node's row, **Unpair**:
    the node's screen says `PHONE UNPAIRED`, Settings says "Unpaired: the node is
    public again", and the phone's Bluetooth settings no longer list the
    node; the log shows `Pairing: the phone forgot its bond with … too`.
    Within a round or two the phone goes on an errand at the node as a
    guest. On a phone that keeps the bond, a dialog says to forget the
    node in the phone's Bluetooth settings, and **Bluetooth settings**
    opens them; the first screen says "ID … no longer knows this phone's
    old pairing", and the line goes within a round of forgetting it
    there, after which the phone uses the node as a guest.
17. Two paired nodes, A and B. Make A the home, then pair with B in
    Settings (**Pair** on its row in the scan if public, a tap on it if
    private). Settings lists both under "Paired
    with", A first as "Your home". Switch B off and take the phone to A:
    within a round the first screen says "Connected to your home, ID …",
    A's ID, never "as a guest". With A switched off and B on, it says
    "Connected to ID …, paired. Home at ID …", B's ID then A's, within a
    round, without the 30 s wait. A phone updated from an app that kept one
    paired node, B, and still holding A's bond, connects to A as its
    owner in its first round there and lists it as paired again.
    **Disconnect**, below the list: both rows say "Not looking. Tap to
    connect", and a tap on A's connects to it, within a scan when it was
    not heard in the last 30 s.
18. A tap on a public node from the home. On the home, with a public
    node C in reach that the phone is not paired with, **Settings**,
    **Scan**, a tap on C's row: the line says "Connected to ID … as a
    guest, until the node ends a guest's link after 30 s", C's ID, the
    node logs the phone's Recipient, and the log has no `Found …,
    connecting` until C ends the link about 30 s after it began; then
    the phone is back on the home within a round. **Pair** on C's row,
    **Pair**: Android asks for the code on C's screen, and Settings
    lists C under "Paired with"; the phone stays on C, as the person
    chose it, until its link drops.
