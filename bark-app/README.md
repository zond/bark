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
  an identity, the name its owner chose and, when the person has set
  one, their home node's address; without a home it carries no
  address, and they can be written to only once they have written.
- **Contact**: a card with the name this phone gives it, known by its
  identity.
- **Home node**: a node on USB that stays where it is, set in Settings;
  it holds the person's mailbox key, and the person fetches their mail
  from it, when they ask, to the node they are connected to.

## Screens

- **Conversations**, the first screen. One row a conversation, newest
  first, with the last text and, for a sent one, its state. A line at
  the top says whether a node is connected, and, when none is, which
  node the phone is paired with or that it is no longer yours (next
  section); tapping it opens Settings.
  The pen button starts a conversation from the contacts. With a home
  node, the inbox button in the bar, **Fetch my mail**, sends a fetch to
  it, which the app does only when asked, and says "Fetching your
  mail". The button is then off, an hourglass that says it waits for
  the home's answer and how long at most (32 minutes, the spec's
  bounds on the way there and back), until a reply comes, "No new mail"
  if the home has nothing new, or the node gives the fetch back. It
  sends nothing, and says why, when no node is connected or the
  connected node is the home: a fetch goes from another node to the
  home, and the home's own inbox already holds the mail.
  When the person's card has changed and has not gone to their
  contacts, a line says why, "Your card changed: your home node has a
  new address" or "…: you have no home now", with **Send my card**,
  which sends it to every contact, one message each; the line then
  goes. The app sends no card by itself (roadmap D126).
- **A thread.** The texts sent to and received from one identity, in
  order, and a field at the bottom to write the next. Each text's small
  line has its time, then where it went, then its state, as icons the
  text's size whose tooltips give the words: a house, to their home; a
  pin, to where they were; a mailbox, from your mailbox; a clock, not
  yet with the node; one check, taken by the node; two checks in the
  theme's colour, acknowledged by the other person. A given-back text
  says so in words with its wait counting down, and can be sent again
  once the wait is over, or dropped; a refused text says why in words.
  The top line says when the contact last wrote, and, when their card
  names a home, whether from it. A received text has a small filled
  check-mark button, **Acknowledge**; nothing is acknowledged by itself.
  Once pressed, the button turns into one plain check, and the two
  checks appear on the sender's copy.
  A text longer than one message holds, 160 bytes in a box without an
  address, goes as parts of up to 159 bytes, 16 at most (roadmap D137):
  once the text in the field needs more than one, a line under it says
  how many, "2 parts", counted in the bytes that will go, one a letter
  for Cyrillic. Send splits it between two characters, after a space
  near the end of each part, and queues the parts in order. The sent
  text is one bubble with the parts' state on its small line: the usual
  icons while every part is in the same state, else words such as
  "2 of 3 taken"; the two checks once every part is acknowledged. A
  received text in parts is one bubble with the parts that have come,
  in order, and "part 2 of 3 not yet here" in place of one that has
  not, until all are in. **Acknowledge** acknowledges every part that
  has come; a part that comes after the tap brings the button back and
  is acknowledged with the next tap.
  A contact whose first message has come but no
  sealed reply is marked not verified; once it is verified, a text
  that came in a first message has a mark of its own, as anyone could
  have made it. A contact without a home who
  has not written yet cannot be written to: a line says so where the
  field would be. The menu renames the contact,
  sends them your card, or removes them. The bar has **Fetch my mail**
  too, as on the first screen. Texts wait on the phone while no node is
  connected.
- **Contacts**, from the menu. Adding one: scan their card with the
  camera, or paste it, and give a name. An address alone is not
  enough: a text is sealed to the identity on the card.
- **My card**, the QR button. Your identity, the card as a QR code for
  another phone to scan, **Scan their card** to add theirs as in
  Contacts, and the home's address and the card as text to copy and
  send. Without a home it says that people can answer you only after
  you write to them.
- **Settings**, from the menu. The node to connect to (scan, then tap
  one) and the node the phone is paired with, the fake node switch, whether the app is exempt from battery
  optimisation (next section), your name, the home node (make the
  connected node your home if it is not, or none; the dialog says so,
  and on **Go ahead** your new card goes to every contact; **Send my
  card**, as on the first screen; clear the mailbox key of a node that
  holds one and is not your home), your 12
  words (show them, or restore an identity from them), and the node's
  map point: set from typed degrees or the phone's location, or
  cleared.
- **Debug**, from the menu. Your identity, card address, home node and
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

While it is not connected, the app looks for the node it is paired with
and connects to it when it sees it, by the same path as a tap and without
a code: from its start, and after a link drops, so that texts waiting go
out as soon as the node is in reach. The first screen, and Settings under
the scan rows, say "Looking for bark-…", or that Bluetooth is off. A
search round scans for four of the node's advertising intervals, 4.1 s,
without a break, and a round starts every 15 s: Android refuses an app's
sixth scan started within 30 s, and two leave room for the person's own
Scan and Pair again. A phone's Bluetooth scanning without a break draws
in the order of 10 mA (an estimate, not measured here), so the search
costs about 3 mAh an hour, under a tenth of a percent of a phone's
battery. The app stops looking while it is connected or Bluetooth is
off, after "This node is no longer yours" (the person decides with
**Pair again**), and after the person presses **Disconnect** or **Stop
looking** in Settings, until they tap a node. That stop is kept with the
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

The queue and the received texts, the contacts, and the person (seed,
name, home, the last `sent`, the mailbox's state) are kept in three
files in the app's private storage, so they survive a restart. The
seed is kept there in the clear: anyone who can read the app's storage
has the identity. Each file names the version of its format, 1 if it
names none; a file of another version, or one with any part that does
not read, is renamed to `<name>.old-<version>` or `<name>.old-bad`, with
`-2`, `-3` and on added if that name is taken, so no file set aside is
ever overwritten; the debug screen's log says so, and the app starts that
file empty (for the person, with a new identity).

## Notifications and the background link

While a node is connected over Bluetooth, the app keeps running with the
screen off or its window closed, under a notification that names the
node, "Connected to bark-…"; tapping it opens the app. The link is the
same one as in the foreground: the app's Dart engine outlives its window
(`Engine.kt`), and an Android foreground service of the connected device
type (`LinkService.kt`) keeps the process running, so there is one
implementation of the link, in Dart. While the app looks for its paired
node (previous section) the service runs too, and its notification says
"Looking for bark-…": without one Android suspends or ends an app in the
background, and the search with it, and queued texts are to go out as
soon as the node is in reach, not when the person next opens the app.
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
the missing ones show. Their card names a home, `bark-fa4e00000002`,
and they write from a node of their own away from it,
`bark-fa4e00000003`, which moves about 110 m north between their
messages. It runs on while the app is disconnected, as
a node does. It counts as paired with the phone; a test can have it
pair with another phone (`FakeNode.pairElsewhere`), and the app then
finds it no longer yours, as with a node.

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
that opens for nobody is kept for the debug screen only.

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
may not know it, so the app knows two addresses for a contact: the
home on their card, if they have one, and the one they last wrote from, with
the age of their newest message, counted from the time it was written,
or from when it arrived if that is earlier. While that age is under a
minute, a text goes where they wrote from without asking, straight to
where they are, with no fetch from their mailbox: in a minute a person
on foot, at a brisk 1.5 m/s, moves about 90 m, within the ≈750 m a
relay hears a handheld in the street (`docs/protocol.md`, "Physical
layer"), while a driver has already left that reach, so no longer wait
makes the guess better. Older, the send
button asks, for that text only: "Where <name> was <age> ago" or "Their
home mailbox". Without a home it goes where they were, without asking;
with a home and nothing written from yet, to the home. A contact with
neither cannot be written to until they write. Acknowledgements and
cards go without asking, to the home's mailbox when there is a choice.

## Plugins

Besides those for Bluetooth, location and storage, pinned packages
handle the QR code, the cryptography and the notifications, all under
licences compatible with the repository's GPL-3:

| package | does | licence |
|---|---|---|
| `sodium` 4.1.2+1 | libsodium 1.0.22, built from its source into the app, and its Dart binding | BSD-3-Clause; libsodium ISC |
| `crypto` 3.0.8 | SHA-256, for the backup words' checksum | BSD-3-Clause |
| `qr_flutter` 4.1.0, with `qr` | draws the code | BSD-3-Clause |
| `qr_code_scanner_plus` 2.3.2 | scans it with the camera | BSD-2-Clause; on Android it uses the ZXing libraries, Apache-2.0 |
| `flutter_local_notifications` 22.3.3 | the texts' notifications and the permission to notify | BSD-3-Clause |

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
the header's written-from address kept apart from the card's, and the
mailbox's replies, and long texts sent as parts, put together in any
order and acknowledged, against the fake node, which answers a fetch
as a home does; pass messages between two phones by hand, in every
order of who scans and who writes first, until both are verified,
and with a first message read after a box, delayed or forged; the
fetch button's wait, ended by a reply, a give-back
or its bound, and after a restart; give the fake node the
mailbox key, read it back, clear it, write it again at every connection
to the home, and refuse a node that does not keep it; group texts into
conversations; split texts at boundaries as the Rust vectors do;
choose where a text goes; decide what notifies, and when the app starts
with the phone, against a notifier that writes down what it is asked;
and drive the screens: a text to the fake friend, its echo, an
acknowledgement, the backup words, a contact from a pasted or a scanned
card, my card, the debug screen, the fetch button, the choice of where
a text goes, and a long text's parts. The Android side, the service and
the boot receiver, cannot run on the host; it is kept small and tried on
a phone (Trying it, With a node).

## Trying it

### With the fake node

1. Open the app. The first screen says no conversations and asks you
   to write down your 12 backup words. Tap the line "Not connected",
   turn on **Fake node**, press **Connect**. The first time, the app
   says why it notifies: **Continue**, and allow it. It shows
   `bark-fa4e00000001`, connected, MTU 247. Further down, **Show my
   words**, write them down, **I wrote them down**.
2. Go back. A notification of the fake friend's two messages has come.
   A conversation with **Fake friend** holds their two
   messages.
3. Menu, **Debug**: your identity and card address; link version 3,
   the node ID `fa4e00000001`, the clock from the phone with the
   phone's offset, Home "not a home", an address, and Status: ready,
   external power, placed, first rotation running with about 3:15 to go,
   the relays known rising from 0 to 3. The log shows the two inbox
   messages read and confirmed.
4. Open the conversation: it says not verified, and the top line "Last
   wrote just now, away from home". Write a text and send it. It shows
   as taken by the node, "to where they were"; 5 s later "Echo: " and
   your text appears, and the note goes: the echo was sealed by the
   fake friend.
5. Press the check-mark button **Acknowledge** on the echo: it becomes
   a plain check mark, and Debug lists an `ack` sent.
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
9. The QR button, **My card**: set a name. Your identity, a QR code
   and the card as text, `BARK:` and capitals, appear, with a line that
   without a home people can answer you only after you write to them.
10. Menu, **Settings**, under **Map point**: press **Phone location**
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
13. Disconnect, close the app, open it again: the conversations, the
    contacts, your name and your identity are still there.

### With a node

1. Switch the node on and read the twelve hex digits on its display.
2. In **Settings**, with **Fake node** off, press **Scan** and allow
   Bluetooth. The node shows within about a second as `bark-` and those
   digits. Tap it. The first time, Android asks for a code and the
   node's screen shows `PAIR CODE` and six digits within a few seconds:
   type them. Settings then says "Connected and paired, MTU 247", and
   the node's screen `PHONE PAIRED`.
3. **Debug** shows link version 3, the node's clock (from its GPS if it
   has a fix, else from the phone, with the phone's offset) and, on a node
   fresh from boot, the
   first rotation counting down from 3:15 (a relay) or 6:30 (a sleepy
   node), and the relays it hears.
4. A node with a GPS fix or a map point shows "Placed: yes" and an
   address. Without one, set a map point in Settings: Status changes to
   placed and map point. A text names the node's address only once it
   has one.
5. With a second node and a second phone, make one phone's node its
   home (step 9 below; a card without a home cannot be written to
   first), then each phone scans the other's card from **My card**,
   **Scan their card** (allow the camera). Send a text from the other
   phone to the one with the home: it shows as taken, and the other phone shows
   it in the sender's conversation within the time the network takes.
   Status's inbox count on the receiving node, in Debug, rises to 1
   and falls to 0 as the app reads it. Answer it: the answer is boxed,
   and the phone that wrote first shows the other as verified. Write
   again from that phone: the phone with the home shows it as verified
   too. Press **Acknowledge** on the answer: the other phone shows a
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
    "Looking for bark-…", then "Connected to bark-…", and texts arrive.
    `adb logcat -s flutter` shows the session's log from the start,
    `Found bark-…, connecting` and `Connected, MTU 247` among it.
    Opening the app shows the link as it stands. After **Disconnect**, a
    restart of the phone starts nothing.
