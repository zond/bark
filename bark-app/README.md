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
  the top says whether a node is connected, "Connected to ID … as a
  guest" during an errand at a public node that is not yours, and, when
  none is, which
  node the phone is paired with or that it is no longer yours (next
  section); tapping it opens Settings. Once there is a contact and
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
  waiting on this phone; two arrows, being handed to the node; one
  check, sent into the network, whose tooltip says the network gives no
  further news until they confirm, as it sends none (spec
  "Addresses"); two checks in the theme's colour, confirmed by the
  recipient. A text the network gave back shows an hourglass and
  "network busy, try again in m:ss", and can be sent again once the
  wait is over, or cancelled; a refused text says why in words. A tap
  on a state opens a sheet that explains them all. The top line says
  when the contact last wrote, and, when their card names a home,
  whether from it. A received text has a labelled chip, **Got it**,
  48 dp tall; nothing is confirmed by itself, and until the person
  first taps one, a line says "Tap Got it so the sender sees you read
  it." Once pressed, the chip turns into one plain check, and the two
  checks appear on the sender's copy.
  A text longer than one message holds, 156 bytes in a box without an
  address, goes as parts of up to 155 bytes, 16 at most (roadmap D137):
  once the text in the field needs more than one, a line under it says
  how many, "2 parts", counted in the bytes that will go, one a letter
  for Cyrillic. Send splits it between two characters, after a space
  near the end of each part, and queues the parts in order. The sent
  text is one bubble with the parts' state on its small line: the usual
  icons while every part is in the same state, else words such as
  "2 of 3 sent"; the two checks once every part is confirmed. A
  received text in parts is one bubble with the parts that have come,
  in order, and "part 2 of 3 not yet here" in place of one that has
  not, until all are in. **Got it** confirms every part that has come;
  a part that comes after the tap brings the chip back and is confirmed
  with the next tap.
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
  read-only, with no field, Got it chips or menu, and
  **Unarchive** at the bottom. An archived contact is still known: its
  card and keys are kept, its messages open, and **Send my card** still
  goes to it. A new text from an archived conversation brings it, and
  its contact, back to the first screen, with the usual notification;
  an acknowledgement or a card does not.
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
  one; a public node says so, with its RSSI, and a tap asks "Is it
  yours?": **Pair**, typing the code its screen shows, or **Just use
  it**, as a guest), the node the phone is paired with, and how the
  connected node fares; on the paired node of link version 7 or later
  the switch **Public: serve phones without pairing**, on unless the
  owner turns it off (Public nodes, below). Then the home node: make
  the connected node your home if it is not, or none; the dialog says
  so, and on **Go ahead** your new card goes to every contact; **Send
  my card**, as on the first screen; clear the mailbox key of a node
  that holds one and is not your home. A node without a place says
  "This node has no place yet", with **Use this phone's location**,
  which gives it the phone's position as its place; a sleepy or
  carried node cannot be made home, and the button is off with "A home
  must be a node that stays on power at a fixed place" (spec
  "Mailbox"). Then the version of the connected node's firmware and
  **Update firmware** (Updating a node, below), whether the app is
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
  relay is a tower, a node on power; a sleepy node a battery, a node on
  battery; a carried one a walker; each in its link's colour;
  the node itself is a target. Tapping one shows its name, RSSI,
  margin, quality, when it was last heard, how far it is from the node,
  and its flags. The map reads Neighbours when it opens, at every
  status the node gives, and when the refresh button is pressed. When
  it opens and when refreshed, on a node of link version 9 or later, it
  also asks each neighbour that has not answered within the last 10 s,
  through Ask, what its own screen shows;
  a neighbour that answers has its role under its marker in its
  screen's words, such as `FIXED RELAY` or `SLEEPY ANCHORED`, and its
  details show its screen's lines one a row under the distance (the
  role, its power, the neighbours it knows, its firmware version and,
  from link version 10, the image it gathers, such as `GET V14 14/31`)
  in place of the sleepy and carried flags they repeat, then this
  node's own view of it, such as its anchor or a ban. An ask or its
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

A node is the paired phone's alone (spec "Phone link", Pairing; roadmap
D135). The first time the phone connects to a node it pairs: Settings
says to type the six-digit code the node's screen shows, and Android
asks for it in its own dialog; Android then takes about five seconds
before it reports the bond, and Settings says "Paired, connecting…"
until the node is read. While a connection is being made no other
starts, and the connected node's row in the scan says "Connected" and
does nothing: a second connect to it would close the link the first
made. Tapping another node's row closes the link and connects to that
one. The bond is the phone's from then on and
every later connection is encrypted without a code. When another phone
has paired with the node since, the node holds no key for this one: it
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
Bluetooth plugin has no connect that takes the address's type. The node paired with, and
whether it was lost, are kept with the queue and shown on the first
screen and in Settings. A pairing cancelled or with a wrong code says
so. Pairing is the Bluetooth plugin's (`UniversalBle.pair`, Android's
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

While it is not connected, the app looks for the node it is paired with
and connects to it when it sees it, and, when it is not paired with a
node, it is no longer its own, or the paired node is not heard in a
round, goes on its errands at the public node heard strongest, by the
best RSSI each node had over the round. The paired node always comes
first, and the app moves to it once it hears it. Each errand takes the
strongest public node of its round, so the phone moves to a nearer one
as it walks. The
app never joins as a guest the node that is no longer its own: Android
still holds the old bond and would encrypt the link with it, which the
node ends.

The paired node is connected by the same path as a tap and without
a code; the search runs from the app's start, and after a link drops,
so that texts waiting go out as soon as a node is in reach. It runs
only once Android allows the app Bluetooth: a fresh install scans
nothing and shows no notification until the person presses **Scan** in
Settings, which asks for it. The app checks again when it comes to the
front and when Bluetooth is switched on or off, in case the person
allowed it in Android's settings. Bluetooth switched off ends the link
as a drop does: Android does not report that end itself. The first screen, and Settings under
the scan rows, say "Looking for ID …" or "Looking for a public node…",
or that Bluetooth is off. A
search round scans for four of a node's advertising intervals, 4.1 s,
without a break, less once it hears the paired node, and a round
starts every 15 s: Android refuses an app's
sixth scan started within 30 s, and two leave room for the person's own
Scan and Pair again. A phone's Bluetooth scanning without a break draws
in the order of 10 mA (an estimate, not measured here), so the search
costs about 3 mAh an hour, under a tenth of a percent of a phone's
battery. The app stops looking while it is connected to its paired
node or Bluetooth is off; after "This node is no longer yours" it looks for public nodes
alone (the person decides with **Pair again**); and it stops after the
person presses **Disconnect** or **Stop looking** in Settings, until
they tap a node. That stop is kept with the
queue, so neither a restart of the app nor of the phone undoes it: the
person chose it, and the phone should not start looking behind their
back. A tap on a node, or a scan, ends a search round at once.

The app keeps its own queue and writes Send only while the node says
ready; on the ATT error 0x80, not ready, it waits for ready again.
Whenever Status counts messages in the inbox, it reads the oldest,
stores it, writes its message ID back so the node forgets it, and
repeats. A node whose link version is above the app's is refused, with
a message to update the app. The first rotation's countdown runs on the phone,
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
screen off or its window closed, under a notification that names the
node, "Connected to ID …"; tapping it opens the app. The link is the
same one as in the foreground: the app's Dart engine outlives its window
(`Engine.kt`), and an Android foreground service of the connected device
type (`LinkService.kt`) keeps the process running, so there is one
implementation of the link, in Dart. While the app looks for its paired
node (previous section) the service runs too, and its notification says
"Looking for ID …": without one Android suspends or ends an app in the
background, and the search with it, and queued texts are to go out as
soon as the node is in reach, not when the person next opens the app.
While it looks for a public node alone, or is a guest, the notification
says so: "Looking for a public node", "Connected to ID … as a
guest". Before Bluetooth is allowed there is no search, and no
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
their thread is opened; tapping it opens the thread. Acknowledgements and
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
a node does. It counts as paired with the phone; a test can have it
pair with another phone (`FakeNode.pairElsewhere`), and the app then
finds it no longer yours, as with a node. Public, as it starts, until
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
**Firmware**, says the version the connected node runs; **Update
firmware** opens a screen that takes the image from **Latest release**,
the `bark-fw-<N>.img` of the latest release the app knows of (Newer
releases), signed by the release, or from **Pick a file**, an image
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
**Commit** tries again once the node is on USB. **Reboot into version
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
(spec "Phone link", Updates). So if the node still knows no relay 585.9 s after it
came back, as long as an image from the mesh would listen, the update
screen and a bar at the top of the conversations say that the node hears
no one and that the new firmware may have broken its radio, and how to
bring it back: the release it ran before, `bark-fw-<M>.uf2`, copied to
it over USB after a double tap of its reset button, since the node
takes no older version from the phone; or a newer release, once there
is one. The bar's × hides it until the next update. A node from
before link version 6 cannot be updated this way: Settings says to
update it by USB once (`bark-fw/README.md`, "Flash").

## Newer releases

At start, and once a day while it runs, the app asks GitHub's releases
API, without an account, for the repository's latest release `v<N>`
(`lib/releases.dart`; `README.md`, "Releases"). When it carries an APK
newer than the app, a bar at the top of the conversations says "A newer
app, version N, is available"; a tap opens the APK's download in the
browser, and Android asks before it installs it over the app. When the
connected node runs a version below N, a bar says "Your node runs
version M, version N is available"; a tap opens **Update firmware**
with the release's image downloaded and chosen (Updating a node), and
for a node from before link version 6 the bar says to update it by USB
once. A bar's × hides it until a newer version, the node's for that
node alone. The app learns its own version from the release workflow,
which builds it with `--dart-define=BARK_VERSION=<N>`; a build without
it is a development build, which Debug says, and which never asks for
a newer app. A node of version 0, a bench build flashed by USB, gets no
bar either: any signed image replaces it, and the bench updates it by
USB. Without a network nothing shows, and the app asks again at its
next start. The last answer, its time and the versions hidden are kept
in `release.json`, so the bars show at once at the next start while the
app asks again.

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
one, acknowledgements or a card. A fetch to your home node is sealed with a
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
back, and makes the node the home only if it then says it holds a key;
otherwise it says so and changes nothing. **No home node** while
connected to the home writes 0, and the node forgets the key. Away from
the home the key cannot be taken back: the app forgets the home and says
that the old home holds the key until the person connects to it and
presses **Clear its mailbox key** there, which writes 0. The same
happens to the old home when another node is made the home. Settings
shows that button on any node that holds a key and is not the person's
home, with a line saying it is someone's home, perhaps a former one of
theirs.

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
D132, D162). Acknowledgements and
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
messages, acknowledgements, a reflected message, a forged first message,
the header's written-from address kept apart from the card's, the
recipient tag on every payload, and the
mailbox's replies, and long texts sent as parts, put together in any
order and acknowledged, against the fake node, which answers a fetch
as a home does; pass messages between two phones by hand, in every
order of who scans and who writes first, until both are verified,
and with a first message read after a box, delayed or forged; the
fetch button's wait, ended by a reply, a give-back
or its bound, and after a restart; give the fake node the
mailbox key, read it back, clear it, write it again at every connection
to the home, and refuse a node that does not keep it; make the fake
public, join it as a guest that writes its tag and touches nothing
closed, read a guest's ring items by its tag after the number it keeps
for the node, across links and restarts, close a second guest, end a
guest's link after 30 s, and take turns as Ready;
choose the paired node, else the strongest public node; go on a
guest's errand (connect, tag, read, send, leave) when something waits
to be sent and otherwise once a minute, try again after being closed for
another guest, and move to the paired node once it is heard; read the fake
node's neighbours, in one page and across several, and not from a node
before link version 4; group texts into
conversations; split texts at boundaries as the Rust vectors do;
choose where a text goes; decide what notifies, and when the app starts
with the phone, against a notifier that writes down what it is asked;
and drive the screens: a text to the fake friend, its echo, an
acknowledgement, the backup words, a contact from a pasted or a scanned
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
3. **Settings**, at the bottom, **Debug**: your identity and card address; link version 10,
   the node ID `fa4e00000001`, the clock from the phone with the
   phone's offset, Home "not a home", an address, and Status: ready,
   external power, placed, first rotation running with about 3:15 to go,
   the relays known rising from 0 to 3. The log shows the two inbox
   messages read and confirmed.
4. Open the conversation: it says not verified, and the top line "Last
   wrote just now, away from home". Write a text and send it. It shows
   as sent into the network, "to where they were"; 5 s later "Echo: " and
   your text appears, and the note goes: the echo was sealed by the
   fake friend.
5. Press **Got it** on the echo: it becomes a plain check mark, and
   Debug lists an `ack` sent.
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
10. Menu, **Settings**, under **Node's place**: press **Phone location**
    (allow location), then **Set**: Debug's address changes, and the
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
15. **Settings**, under **Firmware**: the fake runs version 1. **Update
    firmware**, **Latest release** (or **Pick a file** with a signed
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
   digits, as its screen shows them. Tap it: a public node asks whether
   it is yours; **Pair**. The first time, Android asks for a code and the
   node's screen shows `PAIR CODE` and six digits within a few seconds:
   type them. Settings then says "Connected and paired." and how the node
   fares: "Starting up, m:ss" in its first rotation, "Hears N nodes", or
   "Hears no other node yet: texts wait here"; and
   the node's screen, `PHONE UNPAIRED` before, `PHONE PUBLIC`.
3. **Debug** shows link version 10, the node's clock (from its GPS if it
   has a fix, else from the phone, with the phone's offset) and, on a node
   fresh from boot, the
   first rotation counting down from 3:15 (a relay) or 6:30 (a sleepy
   node), and the relays it hears.
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
   too. Press **Got it** on the answer: the other phone shows a
   check mark on its text.
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
10. Pairing with a second phone: disconnect the first, and connect the
    second to the same node. It asks for the code, and the node's screen
    shows a new one; once typed, the node is the second phone's. Connect
    the first again: it says "This node is no longer yours". **Not
    now** leaves it so, and the first screen says so too; **Pair
    again**, at the node, makes it the first phone's again, and the
    second then finds it no longer its own.
11. Starting with the phone: open the app once after installing it,
    exempt it from battery optimisation in Settings, and connect to the
    node. Restart the phone and unlock it, without opening the app.
    Within about half a minute of the unlock the notification says
    "Looking for ID …", then "Connected to ID …", and texts arrive.
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
    --key-id 255`. In **Settings**, **Firmware** says version N.
    **Update firmware**, **Pick a file**, the image: "Version N+1".
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
    app version 1 and "Latest release: N, checked" a moment ago; the
    bar "A newer app, version N, is available" opens the APK in the
    browser, and its × hides it, also after the app is closed and
    opened again. Connected to the node, the bar "Your node runs
    version 1, version N is available" opens **Update firmware** with
    "Version N" chosen, and its × hides it for that node; another node
    of version 1 still shows it. A node of version 0 shows none.
15. A public node, on link version 8 or later, with two phones. On the owner's
    phone, connected, **Settings**, **Public: serve phones without
    pairing** is on from the start, and the node's screen says `PHONE
    PUBLIC`; turned off, it says `PHONE PRIVATE`, and on again `PHONE
    PUBLIC`. On a second phone that has never paired with it, **Scan**
    lists the node as public with its RSSI; or, with no paired node of
    its own in reach, the second phone asks whether the node is its own,
    and after **Just use it** goes on an errand there within a round, its first screen saying "Connected to ID … as a guest" for
    a few seconds, then again once a minute. No pairing dialog shows,
    at the connect or later. Its log shows `Recipient: tag …`, a
    `Guest: item …` line for each item read, and `Disconnected`.
    A text between the two phones' contacts reaches each, and neither
    sees the other's; a text sent to the second phone while it is away
    from the node is read at its next errand there. Taken near its own
    paired node, the guest phone moves to it within a round.
