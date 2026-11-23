# bark

Small radio nodes that carry text messages across a city when the phone
network and the internet are down.

You write in an app on your phone. The phone talks over Bluetooth to a
node, a pocket-sized radio with a small screen. Nodes pass each message
from one to the next until it reaches the person you wrote to. There is
no server, no account and no SIM card: the nodes are the whole network.

**[Read the manual](https://zond.github.io/bark/manual.html)** to set up
a node and the app. The app itself is on the
[project's page](https://zond.github.io/bark), which every node's screen
shows as a QR code.

## How it works, briefly

- **Nodes.** A node is a LILYGO T-Echo, a ready-made device with a
  radio, GPS, a battery and an e-paper screen. It uses 433 MHz, which
  needs no licence in the EU and Ukraine at the power bark sends with.
- **Relays and pocket nodes.** A node on a charger, in a window or on a
  roof, passes on everyone's messages. A node in a pocket runs on its
  battery for a long time by sleeping most of the time, and sends and
  receives through a relay near it.
- **Addresses are places.** Every node knows where it stands. A message
  is sent toward a place, and each node hands it to the neighbour
  nearest to that place. Nothing is registered anywhere.
- **A home node.** People move, so you can make one node your home: it
  stays in one place on power and keeps your messages until you fetch
  them, from anywhere the network reaches.
- **Public nodes.** A café, a library or a school can put up a node
  that anyone nearby can use with the app, without owning one.
- **Private messages.** The app encrypts every message so that only the
  person it is for can read it.

## What you need

- A LILYGO T-Echo for 433 MHz, or a public node near you.
- An Android phone.

## What to expect

bark is built for when ordinary infrastructure is gone, not as a
replacement for your usual messenger.

- **Text only, and unhurried.** Messages are short and can take a while
  when the network is busy. There are no pictures, calls or groups.
- **No delivery receipts.** The app shows when the network has taken
  your message, not when it arrived.
- **Open, not hardened.** Messages are encrypted, but the network
  itself is open: anyone with a radio in range can see which nodes are
  where, and can disturb the nodes around them.
- **A prototype.** It works on our bench and on a few nodes in daily
  use. It has not yet been tried with many people in a real city.

## Status (2026-11-23)

The node software, the Android app and a simulator of a whole city's
network are written, and releases are published; v47 is the latest. A
handful of nodes run it day to day and update themselves over the
radio. Larger trials on real nodes come next.

## The name

Darwin's bark spider (*Caerostris darwini*) of Madagascar builds the
largest orb webs known, across rivers, on lines up to 25 m long, from
the toughest silk measured. A big web from small, cheap threads.

## For developers

- [`DEVELOPING.md`](DEVELOPING.md): the code's layout, how to build it,
  how releases are made, and what to read first.
- [`docs/protocol.md`](docs/protocol.md): the specification.
- [`docs/roadmap.md`](docs/roadmap.md): every decision and its reason.
- [`AGENTS.md`](AGENTS.md): the rules for anyone who changes the
  repository.
