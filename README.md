# bark

Firmware, protocol core and simulator for a civilian resilience mesh on
433 MHz LoRa: off-the-shelf LILYGO T-Echo nodes (nRF52840 + SX1262) that let
people message each other across a city, through an app linked to their node
over Bluetooth, when ordinary infrastructure is gone. Licence-free in the EU
and Ukraine (10 mW effective radiated power, 10% duty cycle).

The network is open and built for infrastructure loss, not against an
adversary. Nothing on the air is authenticated, so anyone with a radio
in range can disrupt the nodes around them, and anyone in range of a
node learns its ID and position. The app encrypts the messages
themselves end to end. `docs/protocol.md`, "What the protocol omits",
says why.

The name is from Darwin's bark spider (*Caerostris darwini*), the Madagascan
spider that builds the largest orb webs known, anchored on bridge lines up to
25 m long across rivers, out of the toughest biological silk measured. A big
web from small, cheap threads.

## Status (2026-11-05)

The protocol core, the simulator and the firmware are written. The
simulator runs a city's collision domain, 28 km² with 270 relays and
20 000 sleepy nodes, at the design's rate and above; what it measured, and
the decisions it led to, are in `docs/roadmap.md`. The firmware runs the
core, a relay on USB, and on battery a relay until its battery is low if
it stands still, sleepy if carried, behind a loader that swaps in new
images; it has booted and run on one T-Echo board, alone. Three boards
are on the bench, on a measurement build: it has measured the radio's
timings, two frames on one channel and a still link's fading
(`docs/bench-*.md`). Next is the protocol on the three boards together
(`docs/roadmap.md`, Phases 2 and 3).

Read, in order:

1. `docs/protocol.md`: the specification. Radio, channels, neighbour
   discovery, routing, sleepy nodes, packet formats, capacity, open items.
2. `docs/roadmap.md`: phases, decisions taken, open questions.
3. `docs/core-interface.md`: the contract between the protocol core and
   its drivers (simulator, firmware).
4. `docs/simulator-design.md`: the simulator.
5. `docs/implementation-research-2026-10-14.md`: what exists in the Rust
   ecosystem, hardware facts, and corrections to the specification.
6. `docs/reception-research-2026-10-15.md`: how the simulator decides
   that a packet near sensitivity is lost.
7. `docs/firmware-adapter-2026-10-15.md`: the loop that runs the core on
   the board, and what happens to frames that arrive while it is busy.

## Layout

```
bark-proto/   no_std protocol core: packet codecs, tables, routing, MAC state
              machine. No I/O of its own: no executor, no radio driver, time
              passed in, no allocation. docs/core-interface.md is its contract.
bark-driver/  no_std loop that drives the core against a radio and a clock;
              the firmware runs it on the board, its tests on the host
              against a fake radio.
bark-link/    no_std: the values of the phone link's BLE service, shared by
              the firmware and the tests (protocol.md, "Phone link").
bark-screen/  no_std: what a node's e-paper shows, the picture built from
              its state; tested on the host (bark-fw/README.md, Display).
bark-app-layer/
              no_std: what the app puts in a payload, its keys, forms,
              records and card, and the home node's mailbox; its
              vectors.json is what the app's tests check (protocol.md,
              "Phone link", "Mailbox").
bark-sim/     discrete-event simulator and scenario runner (std, host).
bark-fw/      T-Echo firmware: embassy-nrf and a vendored lora-phy around
              bark-driver and bark-proto, and the phone link's BLE stack
              (TrouBLE over Nordic's controller).
              Its own Cargo workspace, cross-compiled.
bark-loader/  the loader the board's bootloader starts: swaps in a staged
              firmware image and swaps it back if it does not confirm
              itself. Its own Cargo workspace, cross-compiled.
bark-image/   no_std: the flash layout, the image header and the swap,
              shared by the firmware, the loader and their build scripts;
              the code an update travels in over the mesh and the store
              of its pieces; tested on the host.
bark-app/     Flutter Android app for the phone link, sealing its texts end to
              end (bark-app-layer).
docs/         design notes, research, decisions.
```

Every node runs the same protocol, and what it does follows its power
and its motion. A relay, a node on USB or another supply, forwards a
message to the neighbour closest to the destination's position,
backtracks around gaps, and expects an acknowledgement from every hop.
Overload is refused back toward the sender's app with a time to try
again; nothing the network has taken is dropped for its age. A sleepy node, on battery,
heartbeats rarely to the relay it has chosen, which holds its messages
until it listens. The protocol core has no external routing dependency (roadmap D1).

## Building

Host (the core, the simulator and the image's layout and swap), stable
Rust:

```
cargo test --workspace
```

The firmware and the loader, each its own workspace, so that the host
workspace never cross-compiles:

```
rustup target add thumbv7em-none-eabihf
(cd bark-fw && cargo build --release)
(cd bark-loader && cargo build --release)
```

The firmware is built protected, which needs in `BARK_SIGNING_KEYS`
the public keys of whoever signs the images it will stage before
`scripts/uf2.py` packs it, or open, for the bench, with `BARK_OPEN=1` (`bark-fw/README.md`, "Read-out protection" and
"Updates").

See `bark-fw/README.md` for packaging the UF2, with the loader, and
flashing.

`.github/workflows/ci.yml` runs the checks every commit must pass
(`AGENTS.md`, "Commits"), with the firmware's and the app's, on every
push to main and every pull request.

## Releases

A tag `v<N>`, N a positive integer, makes release N:
`.github/workflows/release.yml` builds the firmware, the loader, the
measurement console and the Android app, and attaches them to a GitHub
Release whose notes say which file is for what. N is both the
firmware's image version and the app's version, and a board or a phone
takes an update only above the version it runs, so each tag is above the
last:

```
git tag v1 && git push origin v1
```

The app is signed with a release key held in three repository secrets.
Android takes the key as the app's identity, so it is made once and kept,
outside the repository and backed up: an app signed with another key
cannot be updated, only uninstalled and installed again. To make it and
store it:

```
keytool -genkeypair -keystore bark-release.jks -alias bark \
    -keyalg RSA -keysize 4096 -validity 10000
base64 -w0 bark-release.jks | gh secret set ANDROID_KEYSTORE_BASE64
gh secret set ANDROID_KEYSTORE_PASSWORD   # the password given to keytool
gh secret set ANDROID_KEY_ALIAS --body bark
```

The keystore keytool makes has one password for the store and the key,
and the workflow uses it for both.

The firmware image a node stages is signed too, with a key of its own
in the secret `BARK_SIGNING_KEY`, and the firmware is built with its
public key from the repository variable `BARK_SIGNING_KEYS`: a node
stages only an image signed by a key it holds. `bark-fw/README.md`,
"Updates", Keys, says how to make it, where to keep it and how to
replace it.
