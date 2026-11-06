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
  an identity, an address and the name its owner chose. Its address is
  the home node's when the person has set one, else their node's now.
- **Contact**: a card with the name this phone gives it, known by its
  identity.
- **Home node**: a node on USB that stays where it is, set in Settings;
  the person fetches their mail from it, when they ask, to the node they
  are connected to.

## Screens

- **Conversations**, the first screen. One row a conversation, newest
  first, with the last text and, for a sent one, its state. A line at
  the top says whether a node is connected; tapping it opens Settings.
  The pen button starts a conversation from the contacts.
- **A thread.** The texts sent to and received from one identity, in
  order, and a field at the bottom to write the next. A sent text
  shows as queued, taken by the node, or given back with its wait
  counting down; a given-back text can be sent again once the wait is
  over, or dropped; and "acknowledged" once the other person presses
  **Acknowledge** on it. A received text has that button; nothing is
  acknowledged by itself. A contact whose first message has come but no
  sealed reply is marked not verified. The menu renames the contact,
  sends them your card, or removes them. Texts wait on the phone while
  no node is connected.
- **Contacts**, from the menu. Adding one: scan their card with the
  camera, or paste it, and give a name. An address alone is not
  enough: a text is sealed to the identity on the card.
- **My card**, the QR button. Your identity, the card as a QR code for
  another phone to scan, and the address and the card as text to copy
  and send. Without a home node it needs the node's address, kept from
  the last connection.
- **Settings**, from the menu. The node to connect to (scan, then tap
  one), the fake node switch, your name, the home node (make the
  connected node your home, or none; your new card then goes to every
  contact; fetch your mail, which the app does only when asked), your 12 words (show them, or restore
  an identity from them), and the node's map point: set from typed
  degrees or the phone's location, or cleared.
- **Debug**, from the menu. Your identity, card address, home node and
  mailbox state; Node, every Status field, the last Given back value,
  each sent and received message's kind, `sent`, ID and payload in
  hex, and a log of the link's events since the app started.

The app keeps its own queue and writes Send only while the node says
ready; on the ATT error 0x80, not ready, it waits for ready again.
Whenever Status counts messages in the inbox, it reads the oldest,
stores it, writes its message ID back so the node forgets it, and
repeats. A node whose link version is above the app's is refused, with
a message to update the app. The first rotation's countdown runs on the phone,
since the node does not notify it. On every connection to a node of link
version 2 or later the app writes the phone's UTC time and offset to Clock,
silently, and the debug screen shows what the node then has.

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

## The fake node

A switch in Settings replaces Bluetooth by a node written in Dart
(`lib/fake_node.dart`) that answers as the spec says, with an invented
network around it: a first rotation of 195 s in which it hears three
relays; every third message it takes is given back with a wait of
16 bans, 26 s; every fourth write is answered not ready for one ban,
1.58 s. Behind it is one person, the **fake friend**, with an identity
of their own: they start with two first messages to you in the inbox,
with their card, and answer each text you send them with a sealed
"Echo: " of it 5 s later. It runs on while the app is disconnected, as
a node does.

## The payload

What the app sends is sealed with libsodium (the `sodium` package,
which builds libsodium itself into the app), in the forms of spec
"Phone link": a first message to someone in a sealed box with your
card, every later one boxed to their identity, which names no sender:
the app tries its contacts' keys on each message that arrives. A
message names your address while the contact may not know it, carries
the time it was written as its `sent`, and holds a text, an
acknowledgement or a card. A fetch to your home node is sealed with a
key from your identity that only your phones and your home node have,
and the node's replies are opened with it (spec "Mailbox"). A payload
that opens for nobody is kept for the debug screen only.

The home node does not yet hold the mailbox key: the phone link's Home
characteristic is still to be served by the firmware. Until then a
fetch reaches the home node and is kept there as any message.

## Plugins

Besides those for Bluetooth, location and storage, pinned packages
handle the QR code and the cryptography, all under licences compatible
with the repository's GPL-3:

| package | does | licence |
|---|---|---|
| `sodium` 4.1.2+1 | libsodium 1.0.22, built from its source into the app, and its Dart binding | BSD-3-Clause; libsodium ISC |
| `crypto` 3.0.8 | SHA-256, for the backup words' checksum | BSD-3-Clause |
| `qr_flutter` 4.1.0, with `qr` | draws the code | BSD-3-Clause |
| `qr_code_scanner_plus` 2.3.2 | scans it with the camera | BSD-2-Clause; on Android it uses the ZXing libraries, Apache-2.0 |

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
messages, acknowledgements, a reflected message, a forged first message
and the mailbox's replies against the fake node; group texts into
conversations; and drive the screens: a text to the fake friend, its
echo, an acknowledgement, the backup words, a contact from a pasted
card, my card and the debug screen.

## Trying it

### With the fake node

1. Open the app. The first screen says no conversations and asks you
   to write down your 12 backup words. Tap the line "Not connected",
   turn on **Fake node**, press **Connect**: it shows
   `bark-fa4e00000001`, connected, MTU 247. Further down, **Show my
   words**, write them down, **I wrote them down**.
2. Go back. A conversation with **Fake friend** holds their two
   messages.
3. Menu, **Debug**: your identity and card address; link version 2,
   the node ID `fa4e00000001`, the clock from the phone with the
   phone's offset, an address, and Status: ready, external
   power, placed, first rotation running with about 3:15 to go, the
   relays known rising from 0 to 3. The log shows the two inbox
   messages read and confirmed.
4. Open the conversation: it says not verified. Write a text and send
   it. It shows as taken by the node; 5 s later "Echo: " and your text
   appears, and the note goes: the echo was sealed by the fake friend.
5. Press **Acknowledge** under the echo: it shows "you acknowledged
   it", and Debug lists an `ack` sent.
6. Send three more. The third is given back with a wait counting down
   from 0:26. When the wait is over, press **Send again**. This is the
   fourth write, which the fake answers not ready: the text stays
   queued for about two seconds, then it is taken.
7. The QR button, **My card**: set a name. Your identity, a QR code,
   the address and the card as text, `BARK:` and capitals, appear.
8. Menu, **Settings**, under **Map point**: press **Phone location**
   (allow location), then **Set**: the address on My card changes.
   **Clear** brings back the fake's own fix.
9. Disconnect, close the app, open it again: the conversations, the
   contacts, your name and your identity are still there.

### With a node

The firmware's side of the link is still to build (`docs/phone-link-2026-11-05.md`,
"What comes next"). Once a node serves it:

1. Switch the node on and read the twelve hex digits on its display.
2. In **Settings**, with **Fake node** off, press **Scan** and allow
   Bluetooth. The node shows within about a second as `bark-` and those
   digits. Tap it: "Connected, MTU 247".
3. **Debug** shows link version 2, the node's clock (from its GPS if it
   has a fix, else from the phone, with the phone's offset) and, on a node
   fresh from boot, the
   first rotation counting down from 3:15 (a relay) or 6:30 (a sleepy
   node), and the relays it hears.
4. A node with a GPS fix or a map point shows "Placed: yes" and an
   address, and My card shows the QR code. Without one, set a map
   point in Settings: Status changes to placed and map point, and the
   card appears.
5. With a second node and a second phone, each phone scans the other's
   card from **Add contact**, **Scan their card** (allow the camera).
   Send a text from one: it shows as taken, and the other phone shows
   it in the sender's conversation within the time the network takes.
   Status's inbox count on the receiving node, in Debug, rises to 1
   and falls to 0 as the app reads it. Answer it: the first phone's
   contact turns verified. Press **Acknowledge** on the answer: the
   other phone marks its text acknowledged.
6. Switch off the receiving phone's Bluetooth, send two texts to it,
   switch it back on and connect: both arrive, oldest first, and the
   inbox count goes back to 0.
7. Given back and not ready happen only when the network refuses; they
   show as in step 6 with the fake node.
