# bark-app

A plain Android messaging app over a bark node: it talks to the node
over Bluetooth LE with every part of the phone link
(`docs/protocol.md`, "Phone link"; `docs/phone-link-2026-11-05.md`).
It is a prototype for the bench, not the app people will carry.

## Words

- **Node ID**: the radio's 6-byte number, shown on the node's display.
- **Address**: a node's position and node ID, 13 bytes, written as 26
  hex digits. It is what a message is sent to.
- **Card**: what two people exchange so that each can reach the other:
  an address and the name its owner chose.
- **Contact**: a card with the name this phone gives it.
- **Identity**: the person's key. Not yet in the app.

## Screens

- **Conversations**, the first screen. One row a conversation, newest
  first, with the last text and, for a sent one, its state. A line at
  the top says whether a node is connected; tapping it opens Settings.
  The pen button starts a conversation from the contacts.
- **A thread.** The texts sent to and received from one node, in
  order, and a field at the bottom to write the next. A sent text
  shows as queued, taken by the node, or given back with its wait
  counting down; a given-back text can be sent again once the wait is
  over, or dropped. Texts wait on the phone while no node is connected.
- **Contacts**, from the menu. Adding one: scan their card with the
  camera, or paste their card or their address, and give a name. A
  contact is its node: a card scanned again after the node moved
  replaces the old one, and the conversation stays together.
- **My card**, the QR button. The card as a QR code for another phone
  to scan, and the address and the card as text to copy and send. It
  needs the node's address, kept from the last connection.
- **Settings**, from the menu. The node to connect to (scan, then tap
  one), the fake node switch, my name, and the node's map point: set
  from typed degrees or the phone's location, or cleared.
- **Debug**, from the menu. Node, every Status field, the last Given
  back value, each sent and received message's ID and payload in hex,
  and a log of the link's events since the app started.

The app keeps its own queue and writes Send only while the node says
ready; on the ATT error 0x80, not ready, it waits for ready again.
Whenever Status counts messages in the inbox, it reads the oldest,
stores it, writes its message ID back so the node forgets it, and
repeats. A node whose link version is above the app's is refused, with
a message to update the app. The first rotation's countdown runs on the phone,
since the node does not notify it.

The queue and the received texts are kept in one file in the app's
private storage, the contacts, the person's name and the node's last
address in another, so they survive a restart.

## The fake node

A switch in Settings replaces Bluetooth by a node written in Dart
(`lib/fake_node.dart`) that answers as the spec says, with an invented
network around it: a first rotation of 195 s in which it hears three
relays; every third message it takes is given back with a wait of
16 bans, 26 s; every fourth write is answered not ready for one ban,
1.58 s; each message it keeps is answered by an echo in its inbox 5 s
later; it starts with two messages of its own in its inbox. It runs on
while the app is disconnected, as a node does.

## Who sent a text: not yet known

The spec puts the sender's address inside the payload, so that the
relays do not learn it (spec "Phone link", "The payload"). This
prototype's payload does not carry it, so **a text received from a
real node has no known sender**: it goes into one conversation,
"Sender unknown", which cannot be answered. The note on the app's
payload, being written, adds the sender and resolves this.

The fake node knows whom it echoes, and the app asks it: an echo goes
into the conversation of the address it was sent to, and the fake's
own two messages into one named by the fake's address. A conversation
with an address that is not a contact is named by that address until
the person names it, which makes it a contact.

## The payload: not encrypted

The network carries a payload of up to 227 bytes that only the apps
read. End-to-end encryption is open item O1, so this prototype sends
**plain text that anyone in radio range of any node on the way can
read**. Do not send anything private with it.

The payload is:

| bytes | |
|---|---|
| 12 | a random nonce |
| 0 to 215 | the text, UTF-8 |

The spec requires every payload to be unique, since the network names a
message by a hash of its destination and payload, and two messages with
the same name are one to every relay that remembers the first. The
random nonce makes them unique. It is 12 bytes, 96 bits, the nonce of
the common authenticated ciphers, so the encryption can keep this
layout. A received payload that is not in this form is shown in hex.

## The card

A card is a version byte, 1; the 13-byte address; and the name, UTF-8,
to the end. As text, in the QR code and to paste, it is base64url: a
scanner hands a QR code's content over as text, and may change bytes
that are not text. A card of a version above the app's is refused with
a message to update the app, so that the identity key can be added
under a new version (`lib/card.dart`).

## Plugins

Besides those for Bluetooth, location and storage, two pinned plugins
handle the QR code, both under licences compatible with the
repository's GPL-3:

| plugin | does | licence |
|---|---|---|
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
`bark-link/src/tests.rs`, the message ID among them; run the app's
queue and inbox against the fake node; check the card's bytes, its
text and what is refused as not a card; group texts into
conversations; and drive the screens against the fake node: a text
from a thread, its echo back in the same thread, contacts added from a
pasted card and a pasted address, my card, and the debug screen.

## Trying it

### With the fake node

1. Open the app. The first screen says no conversations and no node.
   Tap the line "Not connected", turn on **Fake node**, press
   **Connect**: it shows `bark-fa4e00000001`, connected, MTU 247.
2. Go back. A conversation named by the fake's address, 26 hex digits,
   holds its two messages.
3. Menu, **Debug**: link version 1, the node ID `fa4e00000001`, an
   address, and Status: ready, external power, placed, first rotation
   running with about 3:15 to go, the relays known rising from 0 to 3
   as it counts down, the inbox 0. The log shows the Node read, each
   Status, and the two inbox messages read and confirmed.
4. The QR button, **My card**: set a name. A QR code, the address and
   the card as text appear. Copy the card.
5. Menu, **Contacts**, **Add contact**: paste the card; the name fills
   in. Save: the conversation of step 2 now carries that name, and the
   app opens it. (Scanning needs a second phone showing its card; the
   **Name** button in a thread names an address without a card.)
6. Write a text and send it. It shows as taken by the node; 5 s later
   "Echo: " and your text appears in the same thread.
7. Send two more. The second is taken and echoed; the third is given
   back with a wait counting down from 0:26. When the wait is over,
   press **Send again**. This is the fourth write, which the fake
   answers not ready: the text stays queued for about two seconds,
   then it is taken. The conversation list shows each last text and
   its state.
8. Menu, **Settings**, under **Map point**: press **Phone location**
   (allow location), then **Set**: the line above says the position is
   the map point, and the address on My card changes. **Clear** brings
   back the fake's own fix.
9. Disconnect, close the app, open it again: the conversations, the
   contacts and your name are still there.

### With a node

The firmware's side of the link is still to build (`docs/phone-link-2026-11-05.md`,
"What comes next"). Once a node serves it:

1. Switch the node on and read the twelve hex digits on its display.
2. In **Settings**, with **Fake node** off, press **Scan** and allow
   Bluetooth. The node shows within about a second as `bark-` and those
   digits. Tap it: "Connected, MTU 247".
3. **Debug** shows link version 1 and, on a node fresh from boot, the
   first rotation counting down from 3:15 (a relay) or 6:30 (a sleepy
   node), and the relays it hears.
4. A node with a GPS fix or a map point shows "Placed: yes" and an
   address, and My card shows the QR code. Without one, set a map
   point in Settings: Status changes to placed and map point, and the
   card appears.
5. With a second node and a second phone, each phone scans the other's
   card from **Add contact**, **Scan their card** (allow the camera).
   Send a text from one: it shows as taken, and the other phone shows
   it within the time the network takes, under "Sender unknown" (see
   "Who sent a text"). Status's inbox count on the receiving node, in
   Debug, rises to 1 and falls to 0 as the app reads it.
6. Switch off the receiving phone's Bluetooth, send two texts to it,
   switch it back on and connect: both arrive, oldest first, and the
   inbox count goes back to 0.
7. Given back and not ready happen only when the network refuses; they
   show as in step 7 with the fake node.
