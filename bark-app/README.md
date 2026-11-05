# bark-app

A plain Android app that talks to a bark node over Bluetooth LE and uses
every part of the phone link (`docs/protocol.md`, "Phone link";
`docs/phone-link-2026-11-05.md`): it finds and connects to a node, shows
its status, sends texts, receives them, and sets the node's map point.
It is a prototype for the bench, not the app people will carry.

## What it does

- **Connection.** Scans for the service UUID and lists each node by its
  name, `bark-` and the twelve hex digits of its ID, as the node's
  display shows them. On connect it asks for an MTU of 247 and reads
  Node. A node whose link version is above the app's is refused, with a
  message to update the app: the values the app writes change with the
  version, so it cannot write to a newer node.
- **Node and Status.** The link version, the firmware image, the node's
  address, and every field of Status, live. The address is the
  position of the node's address and its ID, 26 hex digits, with a copy
  button: what a person gives others to reach them. The first
  rotation's countdown runs on the phone, since the node does not
  notify it.
- **Send.** A destination address pasted as 26 hex digits (spaces,
  colons and dashes are ignored) and a text. The app keeps its own
  queue and writes Send only while the node says ready; on the ATT
  error 0x80, not ready, it waits for ready again. Each message shows
  as queued, offered, taken, or given back with its wait counting down;
  a given-back message can be sent again once the wait is over, or
  dropped. A message's ID is computed as the nodes compute it, and
  shown beside it.
- **Inbox.** Whenever Status counts messages in the inbox, the app reads
  the oldest, stores it on the phone, writes its ID back so the node
  forgets it, and repeats until the inbox is empty.
- **Map point.** Set from a typed latitude and longitude, or from the
  phone's location, or cleared.
- **Fake node.** A switch replaces Bluetooth by a node written in Dart
  (`lib/fake_node.dart`) that answers as the spec says, with an invented
  network around it: a first rotation of 195 s in which it hears three
  relays; every third message it takes is given back with a wait of
  16 bans, 26 s; every fourth write is answered not ready for one ban,
  1.58 s; each message it keeps is answered by an echo in its inbox
  5 s later; it starts with two messages in its inbox. It runs on while
  the app is disconnected, as a node does.

The queue and the received messages are kept in a file in the app's
private storage, so they survive a restart.

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

The tests check every value's bytes against the vectors of
`bark-link/src/tests.rs`, the message ID among them, and run the app's
queue and inbox against the fake node.

## Trying it

### With the fake node

1. Open the app, turn on **Fake node**, press **Connect**. You see the
   node `bark-fa4e00000001`, MTU 247, link version 1, and an address of
   26 hex digits. Status says ready, external power, placed, first
   rotation running with about 3:15 to go, and the relays known rise
   from 0 to 3 as it counts down.
2. Under **Received** two messages appear at once, from the fake's
   inbox; the inbox count in Status is 0.
3. Copy the address, paste it into **To**, type a text, press **Queue**.
   It shows as taken; 5 s later "Echo: " and your text appears under
   Received.
4. Queue two more. The second is taken and echoed; the third is given
   back with a wait counting down from 0:26, and Status says not ready.
5. When the wait is over, press the replay button. This is the fourth
   write, which the fake answers not ready: the message stays queued
   for about two seconds, then it is taken.
6. Under **Map point**, press **Phone location** (allow location), then
   **Set**: Status says the position is from a map point and the
   address changes. **Clear** brings back the fake's own fix.
7. Disconnect, close the app, open it again: the queue and the received
   texts are still there.

### With a node

The firmware's side of the link is still to build (`docs/phone-link-2026-11-05.md`,
"What comes next"). Once a node serves it:

1. Switch the node on and read the twelve hex digits on its display.
2. In the app, with **Fake node** off, press **Scan** and allow
   Bluetooth. The node shows within about a second as `bark-` and those
   digits. Press it.
3. You see "Connected, MTU 247" and link version 1. On a node fresh from
   boot Status shows the first rotation counting down from 3:15 (a
   relay) or 6:30 (a sleepy node), and the relays it hears.
4. A node with a GPS fix or a map point shows "Placed: yes" and an
   address. Without one, set a map point: Status changes to placed and
   map point, and the address appears.
5. With a second node and a second phone, give each phone the other's
   address. Send a text from one: it shows as taken, and the other
   phone shows it under Received within the time the network takes.
   Status's inbox count on the receiving node rises to 1 and falls to
   0 as the app reads it.
6. Switch off the receiving phone's Bluetooth, send two texts to
   it, switch it back on and connect: both arrive, oldest first, and
   the inbox count goes back to 0.
7. Given back and not ready happen only when the network refuses; they
   show as in steps 4 and 5 with the fake node.
