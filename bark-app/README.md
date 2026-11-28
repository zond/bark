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
- **Identity file**: a file, locked by a password, that holds the
  identity and what goes with it, to move to a new phone (The identity
  file, below).
- **Card**: what two people exchange so that each can reach the other:
  an identity, the name its person chose, their home node's address
  when the person has set one, and, unless it is the home, the
  address of the node their phone is linked to, or was last, where to
  reach them now, with when it was last linked to it (roadmap D162).
  Without a home the card is temporary: the node on it keeps their mail
  only while they come back to it.
- **Contact**: a card with the name this phone gives it, or else the
  card's own, known by its identity. A contact with neither is listed
  as "Unnamed" with the first eight characters of its identity until a
  card with a name comes or the person renames it.
- **Home node**: a node on USB that stays where it is, set in Settings;
  it holds the person's mailbox key, and the person fetches their mail
  from it, when they ask, to the node they are connected to. A node is
  the home of up to eight people.
- **Owner**: the one person at a home who chooses who else has a home
  there, from anywhere (spec "Phone link", The node's owner): the first
  to make the node their home, or whom an owner or a phone at the node
  made one. A phone with a bond to a node is **paired** with it; that
  makes it no owner.

## Languages

The app speaks English, Swedish and Ukrainian: the first of the phone's
languages it has, else English.

- **Where the words are.** `lib/l10n/app_en.arb` holds every message a
  person reads, and `app_sv.arb` and `app_uk.arb` the same messages by
  the same names. `flutter pub get` makes `AppLocalizations` of them
  (`l10n.yaml`); the made files are kept in the repository.
  `test/l10n_test.dart` fails a translation that lacks a message or takes
  other values than the English.
- **How code reads them.** `tr`, in `lib/l10n.dart`, is the words in the
  language now spoken: `Text(tr.settings)`. The screens and the parts
  with no screen, a notification or a status line, read the same object.
  It is English until `main` tells it the phone's languages, so tests
  read English. A change of the phone's language while the app runs
  builds the screens again from the first.
- **A message is whole.** One sentence is one message with its values put
  in, `tr.barAppOut(version)`: never a sentence joined from translated
  parts, as languages order words differently. A count takes a plural
  message; Ukrainian has four forms.
- **Names.** A message's name begins with the screen or part it belongs
  to (`conversationsNone`, `barAppOut`); a word many screens share has
  none (`settings`, `undo`). The English file says, under `@name`, what a
  value is and, where a translator could not tell, where the message
  shows.
- **Plurals.** The generator keeps one text for `=1` and `one`, so a
  message has one of them, never both. English and Swedish use `=1` or
  `one` with `other`. Ukrainian has `one`, `few`, `many` and `other`,
  and its `one` also serves 21, 31 and so on, so it carries the number.
- **The log.** A notice is shown in the language spoken and logged in
  English: `english`, in `lib/l10n.dart`, is the words for a log line.
  An error a person may read implements `Spoken`: a screen shows
  `said(e)`, never `'$e'`, and `'$e'` in a log line is English.
- **What stays English.** On the debug screen, the log, the sent and
  received records and the raw values, which are what a fault report
  carries; its headings and labels are translated. The words on the
  node's screen, which the app quotes as they are shown; and the twelve
  backup words.

The words a translation keeps to, so that a thing has one name on every
screen and in the manual (`site/sv/`, `site/uk/`):

| English | Swedish | Ukrainian |
|---|---|---|
| node | nod | вузол |
| relay | relä | ретранслятор |
| sleepy node | sömnig nod | сонний вузол |
| anchor | ankare | якір |
| address | adress | адреса |
| card | kort | картка |
| home, home node | hem, hemnod | дім, домашній вузол |
| Set home | Gör till hem | Зробити домом |
| owner | ägare | власник |
| public node | publik nod | публічний вузол |
| paired, to pair | parkopplad, parkoppla | спарений, створити пару |
| guest | gäst | гість |
| mail | post | пошта |
| Fetch my mail | Hämta min post | Забрати мою пошту |
| text (a message) | meddelande | повідомлення |
| firmware | programvara | прошивка |
| contact | kontakt | контакт |
| conversation | samtal | розмова |
| Settings | Inställningar | Налаштування |
| Debug (the screen) | Felsökning | Налагодження |
| connected | ansluten | підключено |
| reaction | reaktion | реакція |
| carried, fixed | bärs omkring, står still | його носять, стоїть на місці |

Swedish says "du", Ukrainian "ви" in lower case. A translation says a
thing as its language does, not word for word: no passive with the doer
in the instrumental in Ukrainian ("вузол відхилив", not "відхилено
вузлом"), and the genitive of телефон is телефону.

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
  connected. Texts wait on the phone". While the node says it is about
  to install a new version of its firmware it got over the mesh, the
  line adds ", installing version N", and after the node's reboot says
  "ID … is installing version N, back in about a minute" in place of
  looking for it; back on that version, it adds ", updated to version
  N" for a minute (Updating a node, below). Why the phone is on that node,
  or on none, is not in the line: its info button opens a few short
  sentences that say, such as that none of the paired nodes has been in
  reach for 30 s, that they are heard too weakly, or that no node of the
  person's is near.
  Settings' Node section shows the same line and button. Tapping the
  line opens Settings. Once there is a contact and
  until the backup words are written down, a card says "If you lose
  this phone, they are the only way back to your contacts"; a tap opens Settings scrolled to the backup, which
  shows tinted for a moment and is announced.
  A row swiped aside either way archives its conversation, with
  **Undo**; a line at the bottom then counts the archived ones, "2
  archived", and leads to Archived.
  The pen button starts a conversation from the contacts. With a home
  node, the inbox button in the bar, **Fetch my mail**, sends a fetch to
  it, which the app does only when asked, and says "Fetching your
  mail". The button is then off, an hourglass that says it waits for
  the home's answer and how long at most (32 minutes, the spec's
  bounds on the way there and back), until a reply comes, "No new mail"
  if the home has nothing new, or the network finds no way to the
  home, which says "No way found to your home"; a fetch given back busy or with no neighbour goes again by
  itself and the button goes on waiting. With
  no node connected the fetch waits, the button saying "Fetch waits for
  a node", and goes from the next node the phone links to, addressed to
  it. On the home it sends nothing and says "At your home, your mail
  comes by itself": paired there, through Inbox; as a guest there, as
  after a restore or an unpairing, through Guest at each errand, as a
  node gives a guest every item of its ring with the guest's tag, those
  its own record keeps for it among them (`bark-app-layer` `mailbox.rs`,
  `for_tag`).
  A contact who gave the person a home (A home given, below) shows as a
  card, "Ann gave you a home at ID …", with an info button, **Not now**,
  which drops it, and **Use it**.
  When the person's card has changed and has not gone to their
  contacts, a line says why, "Your card changed: new home address" or
  "Your card changed: no home", with **Send my card**,
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
  say when it arrives, as it sends no such news; it only gives back
  what it cannot send (spec "Addresses"). A
  text the node gives back shows why, with Cancel under it (spec
  "Phone link", Given back): busy, an hourglass and "The network is
  busy, sending again in m:ss", sent again by itself once the wait is
  over, also after a restart; no neighbour, a beacon struck through and
  "The node hears no other, sending when it does", sent again by
  itself once Status says ready or counts a neighbour heard, with no
  timer of the app's; no way, a pin struck through and "Found no way
  there", which waits for the person, with **Send again** beside
  Cancel, and a notice "Your text to Ann found no way there." unless
  the thread is on the screen. No way can come minutes after the
  check, and for a text before the last. The app reads Given back at
  every link before it writes anything, as the node empties it when it
  takes the next message, and acts on a give-back for a text it has
  as taken or queued, the latter a write whose answer was lost. The
  node keeps the value until then, so the app keeps the last one it
  read from each node and acts on a value once: one for a text
  cancelled or sent again changes nothing, and a message it never sent
  is told once.
  Send again sends the whole text as a new one, with new `sent` and so
  new IDs, as the relays that searched keep their record of the first;
  a receiver that did get the first shows both. The old parts leave the
  queue at once, whatever their state, and the new text goes at the end
  of the thread, with the reaction the old one had, to where a new text
  would go now, asking where as a new text does. Nothing offers
  to send a busy or no-neighbour text sooner: the node decides when. A
  card, a reaction or a home given back for no way is not sent again
  by itself either: a notice says it found no way to the contact, and
  the person sends it again by choosing it again. A text cancelled
  with some of its parts taken says how many of all its parts went,
  "1 of 3 sent". A text the node
  refuses says "Refused by the node", and Debug gives the node's
  answer. A tap
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
  whether from it. A long press on a received text opens the emoji
  picker, under a line that says the sender sees the reaction on their
  text: first a row of the six you
  react with most, in 48 dp buttons, the one used last first on a tie,
  and thumbs up, heart, laugh, surprise, sad and thanks until you have
  used six; then **Copy text**, as the long press no longer selects the
  words; then a search by name, and every emoji in Unicode's groups,
  scrolled. Each emoji is named in its tooltip. The one picked goes at
  once and shows as a small chip half over the lower edge of the
  received text, on its right corner, and on the sender's copy, on its
  left corner, when it arrives; a second pick replaces it there and
  goes again. The chip's tooltip names it, and a tap on it explains it
  with the bubble's other marks. Nothing answers by itself (roadmap
  D126, D185, D187). A sent text's long press still selects its words.
  A text longer than one message holds, 199 bytes in a box without an
  address, goes as parts of up to 198 bytes, 16 at most (roadmap D137;
  spec "Phone link", Long texts):
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
  have made it. A contact whose card names no home and no node, shared
  before its phone used a node, and who has not written yet, cannot be
  written to: a line where the field would be asks for a new card. A
  home the contact gave shows at the top, as on the first screen. The
  menu renames the
  contact,
  sends them your card, for the home's owner gives them a home there
  with **Share my home node** (A home given, below), archives the
  conversation or the contact (both
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
- **Settings**, from the menu, in the order a person looks for things.
  Copy is short throughout, and the reasons are behind an info button
  where they matter: by the connection line, Home node and the node's
  phones.
  - **Node**: the connection line and its help (as on the first
    screen), how the connected node fares, the nodes the phone is
    paired with, then **Disconnect** while connected or **Stop looking**
    while the search runs, apart from the rows so that Disconnect,
    which only ends the link, is not taken for Unpair; then **Scan** and
    the nodes found. Each found node says its signal as strong, at
    −70 dBm or above, fair, or weak, below −85 dBm, a choice, with the
    number in small print, "public" for a public node, and "needs an
    update" for one the search passes over for its version (Versions,
    below); a scan that
    has found nothing after about 10 s, or was stopped, says "No node
    found nearby" and "Switch one on, or come closer". A tap on a
    public node the phone is not paired with uses it as a guest; its
    **Pair** pairs with it (A guest's home, below). Under "Paired
    with" each paired node, the home first, says "Your home",
    "connected", "not in reach" while the search looks, "Needs an
    update. Tap to try again" for one passed over for its version,
    "Not looking. Tap to connect" once the person has stopped the
    search, or "Does not know this phone", with **Pair again** while
    the app has heard it in the last 30 s and **Forget** (Unpairing,
    below); a tap on that row opens a few sentences on why, someone
    removed the phone, the node lost its memory, or the person moved
    from another phone (The identity file, below), and what each does. A tap on
    another paired node's row connects to it as a paired phone: at once when
    the app heard it in the last 30 s, the span the search takes to
    call a node out of reach, else after a scan of one round, saying it
    was not found if it is not in reach.
  - **Home node**: "None: your card is temporary." or "At" and its
    address, with **Set home** where the node is not
    the home, **Remove home** while one is set, and **Send my card**
    as on the first screen. The dialog names its action on the button,
    **Set** or **Remove**, and says what happens to the
    card: "Your new card goes to your N contacts", "Your card stays the
    same", or, while an earlier change has not gone out, that it still
    waits for **Send my card**; on a public node the phone uses as a
    guest it first says "First you pair: type the code on its screen"
    (A guest's home, below). A node without a place says "This node has
    no place yet, so it cannot be a home. Set it under This node."; a
    sleepy or carried node cannot be made home, the button is off, and
    a line says "A home must stay on power in one place" (spec
    "Mailbox"). On a node that keeps the person's mail and is not
    their home nor one the app is leaving, a line says so with **Forget
    me here** (Making a node the home, below). While an old home is being left, a line says
    "Leaving ID …: it sends you its mail, then forgets you." When the
    last fetch to the home had no answer, a line says "Your home did not
    answer your last fetch." (A home removed, below). With no
    home, **Use my existing home** (Using an existing home, below). For
    the home's owner away from it, "People with a home here", with an
    info button, "You are its owner.", the people as the home last
    listed them and **Show them**, then **Ask again** (The home's owner,
    below).
  - **This node**, on a paired node: the switch **Public**, "Any phone
    near it can text through it", on unless its paired phones turn it off
    (Public nodes, below); **Place**, from its GPS or set by a person,
    set from typed degrees or **Use this phone's location**, which says
    the coordinates it set, or cleared; and the node's phones (Phones
    at the node, below), whose own row has **Unpair**: its dialog says
    "The node and this phone forget each other; texts and contacts
    stay", adds "With no phone left, it turns public" when this is the
    node's last phone, and, with other paired nodes, that the phone
    stays paired with them; on **Unpair** the node forgets its bond and
    ends the link, the app forgets that node and looks for it no more,
    and then has Android remove its bond with the node (Unpairing,
    below).
  - **Updates** (Newer releases, below).
  - **My name** and **Backup**: show the 12 words, or restore an
    identity from them, after a warning that contacts reach it only
    once they have its card. A restore ends the link and drops the home,
    which were the identity replaced's. A new name goes at once to the
    node the phone is paired with, which lists it for its other phones.
  - **My identity**: "Save it to a file to move to a new phone.", with
    **Save to a file** and **Restore from a file**, and an info button
    on what the file holds, that a pairing stays on the old phone, and
    why one identity belongs on one phone at a time. Save asks for a
    password twice, with **Add my messages**, unticked, and the line
    "Use it on one phone at a time.", then opens Android's dialog to
    save the file, named `bark-identity-<date>.bark`; Restore says "This
    replaces the identity, contacts and messages on this phone.", opens
    Android's dialog to pick the file, and asks for its password. A
    refused file says why in the dialog, which stays open: "Wrong
    password, or the file is damaged.", "This is not a bark identity
    file." or "This file is from another version of the app.", each
    with "Nothing changed." A restore ends the link and says "Identity
    restored", adding "Pair again with your nodes." when the old phone
    was paired with some (The identity file, below).

    While the 12 words, the dialog that takes them or an identity
    file's password dialog is on screen, Android leaves the app's window
    out of screenshots, screen recordings and the recent apps' thumbnail
    (`FLAG_SECURE`, set through a channel in `MainActivity.kt`).
  - **Battery**: whether Android may close the link with the screen
    off, with **Let bark run**, which opens Android's dialog to exempt
    the app from battery optimisation (next section).
- **Manual**, from the menu, opens the manual, a page of the project's
  site (`site/manual.html`), in the phone's browser; it needs the
  internet.
- **Nearby nodes**, from the menu: a map. The connected node and every neighbour it
  knows on an OpenStreetMap street map, a line from the node to each
  coloured by the link's quality: green good, amber fair, red fragile,
  by the margin of the link's averaged RSSI over the node's noise floor
  (spec "Phone link", Neighbours), dashed while the node avoids it,
  demoted or banned. A legend says the bands in dB and the icons: a
  relay is a tower; a sleepy node a battery; a carried one a walker; each in its link's colour;
  the node itself is a target. Nodes too close on the screen to tap
  apart, their 48 dp tap targets (Material's least) overlapping, share
  one marker with their count, which the legend explains. It is drawn
  as the first in this order: this node, then relays, then sleepy
  nodes. A node is always named by its ID, and the marker shows nothing
  more. A tap on a shared marker lists its nodes in that order, a row
  each with the ID and the kind, "ID … · relay" (`relay`, `sleepy`,
  `carried`), and a tap on a row opens
  that node's details as a tap on its own marker does. Zooming in
  separates nodes that are near but not at one place. Tapping a node's
  marker shows its ID as the title; the people at a node go on a line
  under its ID, here and in the list's rows, with two fixed labels, each
  left out when empty: "Home: You, Anna" for the person's own home and
  the contacts whose card names it, and "Last seen: Cy, Dee" for the
  contacts last known there, which the details give each with when,
  "Cy 5 min ago"; then one row a fact in plain words, none of its screen's, an icon and
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
  exchanged`), GPS signal (signal bars, its GPS's mean C/N0 as its last
  reading had it, `31 dB-Hz · enough for a fix`, or `· too weak for a fix` under the
  22 dB-Hz below which a node drops a fix (`gpsFixFloorDbHz`, which a test holds to the firmware's `CN0_FLOOR`), spec "Position", roadmap
  D234, `no satellites` for 0, and after it how long ago that reading
  was, such as `, 40 min ago`, unless it was this minute, or `no reading`
  when the node has had none since it started, roadmap D251; read from Status
  again as the map opens and is refreshed, as the node does not notify
  it, and from a neighbour's answer), Version (a price tag, the Material set having no version
  icon, such as `V18 · getting V19 14/32` while it gathers an image)
  and Exchanged (a ticked box) if it has taken a message from this
  node; then this node's own view of it: Anchor, `this node's anchor`,
  for the relay this sleepy node sends through, Demoted or Avoided. Each
  icon is named for a screen reader. A tap anywhere on a row opens a
  few plain sentences on what that fact is, how the node knows it and
  what it means for the person, all in `lib/ui/fact_help.dart`. Its power, neighbours and version
  come from its answer (below); before one, what it is comes from its
  heartbeats. This node's own sheet says the same of it from its
  status, its neighbours counted from the table read. The map reads Neighbours when it opens, at every
  status the node gives, and when the refresh button is pressed. When
  it opens, when refreshed and when a neighbour's details are opened, it
  also asks, through Ask, what its own screen shows: every neighbour, or
  the one opened. A neighbour that answers has its role under its marker
  in its screen's words, and its details the rows its answer gives, the
  image it gathers among them. An ask or its answer is lost now and then
  on a busy channel, and a sleepy node may sleep through it, so a
  neighbour that has not answered within 2 s of its ask is asked again,
  up to three asks in a series while the map is open (spec
  "Neighbour status"). Each ask is a frame on the air, and its answer another, so
  one rule, in `lib/asks.dart`, decides for all three: a neighbour is
  asked unless its series is under way or it answered within the last
  10 s, so a tap during a series, or several in quick succession, sends
  nothing more. An older answer stays shown until a new one replaces it.
  While a neighbour has no answer its details say so where its power
  would be: `Asking it for power, neighbours and version…`, then, once
  the asks ran out, `It did not answer.` (for a sleepy node `It did not
  answer: it sleeps between its windows.`) with an **Ask again** button,
  which starts a new series for it alone, as opening its details again
  does. The rows appear in the open details when the answer comes. This
  node's own details ask nothing. The
  node is drawn at its own position, not its address, so a sleepy node
  with a fix is not drawn on its anchor; a sleepy neighbour with an anchor is drawn
  at its address, the anchor's position, which its heartbeats carry, so
  it shares its anchor's marker; a node without a place of its own has its
  neighbours drawn with no lines, and a line says so.
  **Show where I am**, a button in the bar, asks Android for the phone's
  position, and the first time for the location permission while the
  app is in use; the map opening asks nothing. A blue dot marks it while
  the map is open, and the legend adds "you: your position stays on this
  phone, never sent or kept": it is in the screen alone. The connected
  node, when it has no position of its own, a sleepy node with an
  anchor, whose address is its anchor's, or one with no place yet, is
  then drawn at the dot, as the Bluetooth link puts it within metres of
  the phone, with a thin line to its anchor, and the line says "The node
  has no place of its own: it is drawn where you are."; without the dot
  it is drawn at its address, if it has one. Its details' Place then
  says `its anchor's` and that place, where others reach it. A node with
  a position of its own stays there, apart from the dot. The refresh
  button reads the position again once it is shown. With no permission
  or no fix it says why, "Location permission denied" or "No location: …",
  and the map is as before. The position comes from the geolocator
  plugin the app has for **Use this phone's location**, behind one
  interface for both (`lib/phone_place.dart`), with the foreground
  location permissions it had; no background location. Tiles come from the network and are
  kept on the phone for a year, so an area seen once draws again
  without internet; without either, the markers and lines are drawn on
  a blank background and a line says why. A neighbour with no place
  is drawn with this node, without a line, and a line says how many.
- **Debug**, at the bottom of Settings. Your identity, card address, home node and
  mailbox state; Node, what Home reads, every Status field, its GPS signal among them, the last
  Given back value with why, each sent and received message's kind, `sent`, ID
  and payload in hex, and a log of the link's events since the app
  started, which **Copy the log** copies, oldest line first. Each log line also goes to the Android log, so on the bench
  `adb logcat -s flutter` shows them as, for example,
  `11-06 14:03:27.513  8812  8840 I flutter : bark 14:03:27.512 Disconnected`.

A node keeps up to eight paired phones, all alike (spec "Phone link",
Pairing; roadmap D168), and a phone may be paired with many nodes, and
Android keeps a bond for each (roadmap D166). The phone pairs with a node when the person makes it their
home or presses **Pair** on its row in Settings (A guest's home, below),
or taps a private node: Settings says "Type the code on the node's
screen", and Android asks for it in its own dialog; Android then takes about five seconds
before it reports the bond, and Settings says "Paired, connecting…"
until the node is read. While a connection is being made no other
starts, and the connected node's row in the scan says "Connected" and
does nothing: a second connect to it would close the link the first
made. Tapping another node's row closes the link and connects to that
one. A connection the person asks for holds against the search: from
the close of the old link until the new one is up the search neither
scans nor connects, as a round started by the close would take the
home before the tap's own connect was made (roadmap D179); a connect of
the search's already under way ends first, and the person's follows
(roadmap D194). The bond is the phone's from then on and
every later connection is encrypted without a code. When a phone paired
with the node has removed this one (Phones at the node, below), the
node holds no key for this one: it
ends the link as soon as the phone asks to encrypt with its old key,
which fails the MTU request (Android's status 133, the connection's 5);
a refused first read, or a bond Android has dropped after such a
failure, count the same. A link that never comes up is a plain failure
to connect. The app then says "This node is
no longer yours", "ID … no longer knows this phone. Pair again with the
code on its screen.", with **Not now** and **Pair again**, which forgets the
phone's old bond, scans until it sees the node again (about ten seconds
at most, else it says the node was not found), and pairs anew, at the
node. The scan is needed because the node's address is random static,
and Android, with neither a bond nor a scan result to tell it so,
connects to it as a public address and times out after 30 s; the
Bluetooth plugin has no connect that takes the address's type. The
nodes paired with, and for each whether it was lost, are kept with the
queue and listed in Settings; pairing with one more adds it and keeps
the others. A pairing that ends without one always says why, in a
dialog on the screen where it was started, as the person's eyes are on
the node's screen then: "Not paired: cancelled, a wrong code, or the node
is full (its screen says PHONES FULL)", as Android does not tell them
apart; "The link to ID … ended. Try again nearer to it"; "ID … not
found: come nearer and try again"; or Android kept an old bond with it (Old bonds, below). A
paired phone's link that ends while the app reads it is such an ending, not a
node that holds the bond. Pairing is the Bluetooth plugin's (`UniversalBle.pair`, Android's
bond), which needs no code of the app's own on the Android side.

**Public nodes.** Every node is public unless a paired phone turns the
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
bond. On every connection, a paired phone's or a guest's, the app first
writes the person's recipient tag
at that node to Recipient. A public node keeps every message that is
not its paired phones' in its ring, with the tag it carries, whether or not
its guest is there, until the ring wraps; it keeps nothing for a guest
beyond its link.

**Unpairing.** The node forgets its bond when its phone unpairs, but
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
answer. Where the bond stays, a dialog says "Unpaired", then "Forget
the node in Bluetooth settings too: the phone kept its pairing", with
**Bluetooth settings**, which opens the phone's Bluetooth settings (the intent
`android.settings.BLUETOOTH_SETTINGS`, `MainActivity.kt`), where the
person forgets the node. Either way, if the node was the person's home,
the line adds "It stays your home": the node keeps the mailbox key. The search then uses the node as a
guest; the texts, the contacts, the home and the card stay as they
were.

**Forget**, on the row of a node no longer the phone's, takes the node
from the paired nodes after a dialog, "This phone stops listing it",
adding "It stays your home" for the home, as after Unpair.
Android names a bond by the device address, which the app learns only
by hearing the node in a scan, and does not keep across runs. Where it
has heard the node since it started, it has Android remove the bond as
on unpairing, and where Android keeps it, says so with **Bluetooth
settings**. Where it has not, a dialog says the phone drops its
pairing when it next hears the node (Old bonds, below), or
to forget it in Bluetooth settings now, with the same button.

**Phones at the node.** On a paired phone's link, Settings shows under This
node the node's phones, read from Phones page by page at each
connection, after each removal and whenever Status's `phones` moves,
as another phone pairs, leaves or changes there (spec "Phone link",
Status, Phones). One read runs at a time; one asked for meanwhile makes
it read again, and a read during which `phones` moved is not shown but
read again, so the list is never made of the pages of two. With the
list it reads Node and Home again, for the node's room and whether it
is anyone's home. A removal's dialog whose phone leaves the list while
it is open closes and says "Already gone." "Phones
paired with this node", whose info button says that a node keeps up to
eight phones, that anyone at it can pair by the code it shows and that
every paired phone can change its settings, so a phone not known or
gone is best removed; on a full node "Full: remove one before another
can pair"; then "Phones with a home here", a record that holds both listed in both, marked "has a home here
too" and "paired too". Each row is the person's name as their app wrote
it to Recipient (Versions, below), or "A phone with no name" while its
phone has written none, with "this phone" on the reader's own, "owner"
on the owner's, and "linked now" or the day its person was last seen,
linked or fetching, as "seen today", "yesterday", "N days ago" or
"never", for a node that had no time on those days.
Another's paired row has **Remove**, for what its record holds; a row
under the homes has a menu, **Choices**, with **Remove**, unless it is
this phone's, and **Make owner**, unless it is the owner's: at the node
any paired phone does either, by the code on its screen
(spec "Phone link", Phones). The removal's dialog says
what goes, "Ann's phone can no longer use the node unless paired
again." and "The node stops keeping their mail."; Make owner's, "Make
Ann the owner?", "The owner chooses who has a home here."; each adds
"You type the code the node then shows." **Show the code** asks the
node for one, and its screen shows `CONFIRM` and six digits; the person
types them, "Type the six digits under CONFIRM on the node's screen,
within 30 s.", and **Remove** or **Make owner** writes it, which says
"Removed" or "Ann is the owner". A removed phone's app finds the
node no longer its own at its next link; a removed home tells its
person at their next link there, or at their next fetch from elsewhere
(A home removed, below). A wrong code, or one after 30 s, says "Not removed" or "Not
changed", "Wrong code, or too late. Each code works once.", and offers
**New code**; a record the node no longer holds says "Already gone" and
reads the list again. While no record under the homes is the owner's,
"This node has no owner"; at the person's home it has **Become
owner**, which writes Home again with their key, as a key written takes
the empty seat (spec "Phone link", Settings), and says "You are its
owner". The info button of the lists says what the owner is.
This phone's own paired row has **Unpair**; its own home is changed
under the home node.

**Old bonds.** The app's list of paired nodes is the only word on
which nodes are the person's. Android may list a bond with a node not
on it: one paired with before the app's data was cleared, on another
install or before the 12 words were restored, one no longer the
phone's, or one forgotten. The app has Android remove such a bond as
soon as it hears the node, in a search round or the person's Scan, and
before any link to it, by the same call as on unpairing: Android
encrypts every link to a node it holds a bond with, which the node
takes as a paired phone's or ends (spec "Phone link", Public), and pairs
with its old keys while the node runs a new pairing, so that the key
check fails whatever code is typed (bench, 2026-11-10, DHKey check
failed three times). The node is then a public node like any other,
and the person's again only once they pair with it by the code on its
screen, even where it still held the phone's bond: after the app's data
is cleared, each node takes its code once. With the bond goes what
Android knew of the node's address, so a link to it waits for a scan
that sees it again. Where Android keeps the bond, the first screen and
Settings say once "The phone kept its old pairing with ID …, so it
cannot use the node. Forget it in Bluetooth settings.", with
**Bluetooth settings**, and a tap on the node in the scan, or **Pair**,
says the same in a dialog. The app passes the node over until Android
no longer lists the bond: it checks at each round and when the app
comes to the front, and the line goes once the bond is gone (roadmap
D194, D203).

**A guest's errand.** A public node takes one guest's link at a time
beside its paired phones' and ends it after 30 s, so a guest does not stay. It
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
any node, asking for the turn as below. A phone that connects while another guest has the link is
closed as it writes its tag; the next round tries again.

A guest's link the person makes by a tap on a public node in Settings
is not an errand: the app does not leave when the errand would, and
the search does not run beside it, so it lasts until the node ends it
after its 30 s, or the person goes to another node. The line says so,
"Connected to ID … as a guest for 30 s", and its help that the node
serves one guest at a time. Then the
search chooses again, the home first (roadmap D179).

The app goes on an errand at a public node when it has something to
send, at the next round, and otherwise once a minute (`guestCheck`): a
minute is how long the app takes a person on foot to stay within a
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
**Set home**, its line adding "You pair first, with the
code on its screen". Settings has the same button under the home node, its dialog
saying the same. The link
is down between errands, so the app keeps the node's address, whether
it can be a home and Node's `room` from the last
errand there. A node whose `room` was 0 keeps eight phones already and
would refuse the pairing: the first screen does not offer it, the home
button is off, and a line says "This node is full: eight phones are
paired with it." A pairing that fails, a wrong code or one cancelled,
says why, adding "Still a guest here", and the phone goes on using the
node as a guest.

Whoever keeps a public node, or wants a standing link to it without
making it their home, presses **Pair** on its row in Settings' scan,
shown on each public node the phone is not paired with. Its dialog,
"Pair with ID …?", says "You can then change its settings and update
it. Type the code on its screen." Pairing does not make it the home,
and the phone keeps a link to a paired node while it is in reach
unless the home is too. **Pair** connects and pairs at once
by the code on the node's screen, with no guest's errand first, so it
works while the home is in reach; a pairing that fails says why, and the
search then chooses again. A node whose `room` was 0 at the last errand
says it is full instead (roadmap D179).

One function decides, from what the phone knows of its nodes, its link
and its last scan, whether the search scans and which node it takes as
whose (`decide`, `lib/decision.dart`); the search carries that out and
decides again on what came of it. While it is not connected, the app
looks for the nodes it is paired with and connects to the first of them
it hears, in this order: the
home, when the phone is paired with it; else the paired node heard
strongest, by the best RSSI each node had over the round. When it is
paired with none that is still its own, or they are out of reach, it
goes on its errands at the public node heard strongest (roadmap D163,
D171). The order holds among the nodes heard
at −85 dBm or above, where Settings stops saying "weak"
(`FoundNode.usable`): a home or paired node heard below it is passed
over for a node further down the order heard above it, a public one at
once; with none heard above it, the app takes the node heard strongest,
whatever its kind (`chooseNode`). The paired nodes are out of reach
once two rounds in a row, 30 s, have reached none of them, heard or
connected (`ownGone`): one round with the screen off may miss a node in
reach. After a paired node that said it was installing ends the link,
the search waits for it (Updating a node, below). A paired node heard well always comes first, and the app moves
to it in the first round that hears it, so at the edge of its reach,
heard every other round, the phone stays with it. On a paired node that
is not the home the phone is a paired phone there, with the standing link and the
paired phone's inbox, as on the home. There, while the phone is paired with
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
as it walks. A node no longer the phone's is, once its bond is gone, a
public node like any other (Old bonds, above).

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

The app keeps its own queue and writes Send while the node says ready.
It also writes Send while ready is not set, which asks the node for the turn among its phones (spec
"Phone link", Sending): the node answers the ATT error 0x80, not ready,
unless the turn passes to the phone at that write. On 0x80 the app
waits for ready again, and asks again if ready has not come within
12.64 s (`Session.turnWait`), a ban for each of the node's eight other
links.
Whenever Status counts messages in the inbox, it reads the oldest,
stores it, writes its message ID back so the node forgets it, and
repeats.

**Versions.** The app speaks link version 1 and serves nodes of that
version, `oldestNode` (roadmap D188). The node says in Node the oldest
app version it serves. The app reads Node first and goes on only when
each side's version is at least the other's oldest (spec "Phone link",
Versions; roadmap D177); otherwise it closes the link and says which
node and which side to update: "ID 275C1AB9BAC7 needs a newer app.
Update the app.", or "ID 275C1AB9BAC7 needs newer firmware. Update the
node." The node would refuse the phone every round, so the search then
passes it over, as a node whose bond Android keeps: no round, errand or
move to the home takes it. A paired node so refused counts as none, so
the phone goes on at once to another paired node; with none in reach,
to a public node after the two rounds as before, and with no other
paired node, from the next round. Settings' row of the node, among the
paired nodes or in a scan, says "Cannot be used yet", and a tap on it tries
it again; so does the app started again, as this is not kept. A link to
the node ends it. The phone is by then on another node, so the notice
stays there until the person dismisses it or the phone links to that
node, and as the node is not tried again it does not come back by
itself. To a node it serves, the app then writes Recipient with the
tag, its own link version and `oldestNode`, and the person's name from
their card on a paired phone's link,
cut to 32 bytes at a character's end; a guest writes no name, as the
node keeps nothing of a guest's. A node that finds the two do not serve
each other answers 0x82 and ends the link, and the app says the same.
A newer node that serves the app is used as one of its own version; the
**App** row in Settings' Updates adds, without a notice, "Update it
for the node's new features".
The first rotation's countdown runs on the phone,
since the node does not notify it. On every connection the app writes
the phone's UTC time and offset to Clock,
silently, and the debug screen shows what the node then has.

On every connection the app reads Home. To the person's home, known by its node ID, it first writes the
key and a fresh `sent` (Making a node the home, below), silently, with a line in the log: the node may have been reflashed,
or the identity restored from its words, and a node that holds that key
already changes nothing. When a fixed, awake home's position is not
the one the person has for it, the app takes the new one, which changes
the card: such a node takes a new place at boot or when it turns fixed,
and the home is the node, not the spot. The app does not send the
changed card; the person sends it with **Send my card** (Screens).
A carried or sleepy home keeps the position the person has for it: a
carried node's position follows its GPS fix every second, and a sleepy
node's is its anchor's.

Home says whether the node is this phone's person's home, `mine`, and
the key goes to the home only where Home
says it is not theirs and the node's phones (Phones at the node, above)
list a home of their tag: one a phone of theirs left by unpairing,
which the write joins to this phone's record, or another phone of
theirs. Where none is listed, someone paired with the node may have
removed the home there, and the app tells the person rather than
overrule them (A home removed, below).

The app keeps four files in its private storage, so that they survive
a restart: the queue, the received texts and the nodes in `bark.json`,
the contacts and what is archived in `contacts.json`, the person (seed,
name, home, the last `sent`, the mailbox's state) in `me.json`, and the
last release heard of in `release.json` (Newer releases, below). The
seed is kept there in the clear: anyone who can read the app's storage
has the identity. Each file names the version of its format; a file of
another version or none, or one with any part that does not read, is
renamed to `<name>.old-<version>` or `<name>.old-bad`, with
`-2`, `-3` and on added if that name is taken, so no file set aside is
ever overwritten; the debug screen's log says so, and the app starts that
file empty (for the person, with a new identity). The app reads only
the forms it writes, and `bark.json` of the version before.
`me.json`, `contacts.json` and `release.json` are each one JSON object, written whole
to a file beside it and renamed over it, so that a kill leaves the old
or the new. `bark.json`, which grows with every text, is a log: a first
line `{"version":4}`, then a line for each change, a message queued,
changed, dropped or received, or the nodes and the reactions used, so
that saving costs what changed. A line is written whole or not at all;
a kill mid-line loses that change alone, and the next start writes the
file afresh without it. Once the lines outnumber twice what the file
holds, it is written whole again, a line a message, the same way as the
others. Version 3, before it, held everything as one object; the app
reads it and writes it afresh as a log. A field added to a form that every file before
lacks, such as a home's given key, the owner's mark, the home's newest
payload version or a home being left
in `me.json`, a share and a home given in `contacts.json`, a fetch's
command, why a message was given back, the IDs a text sent again
replaced, or the last Given back read from a node in `bark.json`, is
optional and its absence means none, or for why, busy, the only reason
there was: such a
file reads in full, and the version stays, as a raised one would set
every phone's file aside, its identity with it.
The person can also save it all to an identity
file, and restore it on another phone (The identity file, below).

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
later than 15 s.

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

## The tests' node

The tests run the app against a node written in Dart
(`test/fake_node.dart`), behind a phone's Bluetooth written in Dart too
(`test/fake_ble.dart`), that answers as the spec says, with an invented
network around it: a first rotation of 195 s in which it hears three
relays; every third message it takes is given back busy with a wait
of 16 bans, 26 s, unless a test turns that off; every fourth write is
answered not ready for one ban, 1.58 s. A test may have it give back
any message the phone sent, at any time, for any reason, have a
message's search come back with no way in place of arriving, and lose
its last neighbour, giving back the message it holds and taking none
until it hears one again, or give one back for no neighbour while it
still hears some; it may take a message and end the link before the
phone hears the answer. Its Given back stays across the phone's links
until it takes another message, which empties it without a
notification, as a node's does; and the same ID again after a no way
is taken and goes nowhere, as the relays that searched have it. Behind it is one person, the **fake friend**, with an identity
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
draws a code on `askCode` (`shownCode`), and takes a change by code as
a node does, a removal or the owner's mark: 0x13 for a record or part
it does not hold, 0x81 for another code or one past 30 s, a code tried
once; a key written takes the empty owner's seat (`mineOwner`). Its
home away keeps records of its own (`homeRecords`), the person's first,
its owner, and answers the owner's commands as a home does, saying what
each did (`commands`); a test can have it hear nothing (`homeAnswers`)
or lose the answer to a command it took (`losesAnswer`). Public, as it starts, until
the switch or a test makes it private, it takes one guest's link beside
its paired phones', closes a second guest's as it writes its tag, and ends a
guest's after 30 s; a guest's link answers 0x05, insufficient
authentication, to every value that needs the bond, and 0x13 to a write
to Inbox. It keeps a message in the paired phone's inbox when it carries the
phone's tag, or the phone has written none yet, and the rest in a ring
of 1024 by number, which a guest reads by its tag after the number it
writes to Guest; a test can make a node nobody has paired with
(`hasPhones`), whose messages all go to the ring. Its turns are Ready, set for one phone at a time and
given only to one that wants it: a Send out of turn asks for it and is
answered 0x80, not ready. The turn passes, in
the order of the links, when the node takes the holder's message, when
its link ends, or after a ban unused while another wants it, and stays
otherwise. It takes an update as a node does, and its
new image hears no relay, unless a test says so
(`FakeNode.hearsAfterUpdate`), so that the warning of a node that hears
no one after its update (Updating a node) can be seen.

## Updating a node

A node out of reach, on a roof, is updated from the phone over the link
(spec "Phone link", Updates; roadmap D145). Settings, under
**Updates**, says the version the connected node runs on the **Node
firmware** row; its **Send to the node**, when the latest release has a newer
image, opens the update screen with the image downloaded and chosen, the
`bark-fw-<F>.img` of the latest release the app knows of (Newer
releases), signed by the release that built it; **Download latest
release** fetches it when it is not the image chosen. For the bench, Debug, under
**Bench**, has **Pick a firmware file**, which opens the same screen
with **Pick a file** beside it for an image
`bark-fw/scripts/image.py pack` made and `image.py sign` signed; the
node refuses an image no key of its own signed (`bark-fw/README.md`,
"Updates"). **Download latest release** is offered only when that release's
image is newer than the node's; otherwise a line under the buttons
says why: no release known yet, or the node already runs the latest.
The app checks the file's header and hash before it offers it, and shows its
version, and says so when it is not newer than the node's: the node
would refuse it. **Send to the node** sends it with a progress bar, in
windows of seven pieces of 240 bytes, each window followed by a read of
where the node stands (`lib/updater.dart`); it asks Android for a short
connection interval first, and an image of 400 KB takes about 20 s, up
to a minute where Android does not grant it. If the link is lost, the
screen says how far it got, and when the app connects to that node
again it goes on from the bytes the node holds, not from the start. Once every byte is
there the app commits: the node checks the image and answers, and the
screen says it keeps it for its next boot, or why it refused, in words.
A refusal for a low battery keeps the image on the node, and
**Try again** asks the node to check it again once it is on USB. **Reboot into version
N** then restarts the node, which swaps the image in, about a minute.
The node answers and ends the link, and Android may report the write
failed when the end overtakes the answer; a link that ends after such a
failure is the reboot, not an error. The search then looks for that
node alone: it connects to no other paired node and no public node, so
texts written meanwhile wait on the phone, and the connection line says
"ID … is installing version N, back in about a minute", and "not back
yet" once the swap's 55 s (`swapAway`) are over. It waits as long as
the node's firmware may take, about 12 minutes (`backWithin`): the
swap, an image that hangs until the loader's watchdog resets it after
600 s, and the swap back (`bark-fw/README.md`, "Updates"); then one
more round, as a round with the screen off may miss the node. A node
not back by then is out of the phone's reach or does not boot: the
update screen says it is not back and may be out of reach, and the
search goes on as usual. A node the person taps connects at once, and
their link to another paired node ends the wait. Once the node is
back the app connects to it by itself, and the screen says the node
runs version N, updated from version M just now, or so many minutes
ago; the image, its progress and its buttons go until another is
picked. If the new image did not start, the node swapped the old one
back, and the screen says the version it came back on. Only the node
the image went to decides this, whenever it is back, and only it is
sent the rest of an image or asked to commit or reboot: another node
the app is on meanwhile changes nothing of the update. An image from the phone keeps itself
whatever the node hears, since the node may stand alone
(spec "Phone link", Updates). So if the node still hears no other node,
sleepy ones counted, 585.9 s after it came back, as long as an image
from the mesh would listen, the update screen and a bar at the top of
the conversations say that the node hears no one and that the new
firmware may have broken its radio; the update screen says how to go
back: a double tap of its reset button and the release it ran before,
`bark-fw-<M>.uf2`, copied to it over USB, as the phone cannot send an
older version. A newer release, once there is one, can be sent as
usual. The bar's × hides it until the next update.

A node also installs an image it gathered over the mesh by itself
(spec "Updates over the mesh", the install). From the moment it drains
for the reboot its Status says the version, `installing`. The
connection line then says
"Connected to ID …, installing version N", and its help that the node
passes on the texts it holds, at most ten minutes, then restarts for
about a minute, and that the phone reconnects by itself. When the node ends the link to reboot, the line says "ID … is
installing version N, back in about a minute", and the search waits
for it as for a node the phone updated (above), but only for 55 s
(`swapAway`), from a reboot into an image of 227 pages
to the node back on the bench, 2026-11-06, the loader's hash and swap
of about 47 s by the datasheets (`bark-fw/README.md`, "Updates") and
the boot, and then the first of the two rounds that call a node out of
reach, as a round with the screen off may miss it. Meanwhile it looks
for that node alone: it connects to no other paired node and no public
node, so texts written
meanwhile wait on the phone. The second round is an ordinary one,
which uses a public node if it misses the node too, about 85 s after
the link ended. A node the person taps connects at once, and their
link to another paired node ends the wait (`waitsForInstall`). A node
that comes back on a version that it and the app do not serve ends the
wait at once, and is passed over (Versions, above). A link
the phone itself closes while the node drains, as when it moves to the
home, is no reboot, and nothing is waited for. Back on version N, the
line adds ", updated to version N" for a minute (`updatedShown`). A
node whose drain opens again, for a low battery or a phone's update,
clears `installing`, and the line goes back to what it was (roadmap
D182, D202).

## Newer releases

At start, and once a day while it runs, the app asks GitHub's releases
API, without an account, for the repository's latest release `v<N>`
(`lib/releases.dart`; `DEVELOPING.md`, "Releases"), and downloads its
`release.json`, `{"release": N, "app": A, "firmware": F}`: the app's
version A and the node firmware's F that the release carries, each N
when the release built it or an earlier release's carried on unchanged;
a release without the file is not read. When the release carries an APK
newer than the app, a bar at the top of the conversations says "App
version A is out", "Tap to install". A tap downloads the APK into the
app's cache, the bar saying "Downloading…" and the percent done,
and hands it to Android's installer, which asks the person before it
installs it over the app (`lib/app_update.dart`). A tap while it
downloads does nothing; a failed download says so, and a tap tries
again. Only the latest APK is kept, and only once whole, so a tap after
the installer was left installs it again at once. The first time,
Android sends the person to allow the app to install apps; if the
installer does not come back by itself, a tap on the bar starts it
again. Android refuses an APK signed with another key than the
installed app's (Building). When
the connected node runs a version below F, a bar says "Node firmware F
is out", "Tap to update the node"; a tap opens **Update firmware**
with the release's image downloaded and chosen (Updating a node). A
bar's × hides it until a newer version, the node's for that
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
date (22); node firmware 23 is out: send it below", "App 22 is out: install
it below; no node linked", or "Could not reach the release list"
without a network. Under it the **App** row gives the app's version and,
when the release is newer, **Install**, which downloads and installs it
as the bar does; the **Node firmware** row
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
and the node's replies are opened with it: first one that says how many
items come and how many stay at home, the count the inbox button shows,
then one for each item, which opens as the message it is (spec
"Mailbox"). A payload
that opens for nobody is kept for the debug screen only. Each payload's
second byte, in the clear, is its recipient's tag at the node it goes
to, made from the recipient's identity and that node's ID, by which a
public node tells whose it is (Public nodes, above); a fetch carries
your tag at your home, and inside it your tag at the node the replies
are to come to, which the replies carry. No receiver checks it. The
person's card says, in its flags' high four bits, the newest payload
version this app reads, 2, and a home's summary says its own in its
flags' (spec "Phone link", The payload; roadmap D238); the app keeps a
contact's with their card and the home's with the home in `me.json`.
There is one version, so no choice is made by them yet.

**Making a node the home.** The app writes 1, the mailbox key and a
fresh `sent` to Home, 37 bytes, the next of its own rising count, so
that every later fetch is later than it: a node takes no fetch of a key
that is not later than the write that set it (spec "Phone link",
Settings; roadmap D236). Every write of a key does so, in one place
(`Session.writeHome`, and the silent one at the home). It reads Home
back, and makes the node the home only if it then says it holds the
person's (`mine`); otherwise it says so and changes nothing. A key
written takes the owner's seat if it is empty, so the first person to
make a node their home is its owner. **Remove home** while
connected to the home writes 0, and the node forgets the key.

**Leaving a home.** Away from the old home, when the person chooses Remove
home, makes another node the home or uses a home given, the app
leaves the old one by itself, and says "Your old home sends you its
mail, then forgets you.": it keeps the old home's address and key
(`me.json`, `leaving`), fetches from it, from the node the phone is on
or the next with a place, again while its summary says more stays than
a batch, once every item the summary counted has come or the fetch's
wait (32 minutes) is over; then it sends a leave, a fetch whose command
has the home forget the person's key (spec "Mailbox", The owner's
commands). The leave goes after the mail because a later fetch ends a
round, and a leave's round carries no item. The leave's summary may be
lost, and asked again the home would not answer, as it holds the key no
more; so the leave is sent once, and the old home is forgotten when its
summary comes or its wait is over, the log saying which. A leave that
never reached the home leaves the key there. The app keeps such a home
and its key (`me.json`, `left`), as it does one whose leave was given up
for another, and when the phone is next on that node and the node, read
afresh, says it still holds the key, it takes the leave up again by
itself, once a connection; a node that holds the key no more is
forgotten as left (`Mailbox._leftHere`). A node that says it holds this
phone's key and is neither the home nor one the app left gets no such
leave: the key may be that of an identity that moved to another phone,
whose home it still is. There Settings shows **Forget me here** with
the line "This node keeps mail for this phone, and is not your home",
which writes 0 at the node. A node that holds another person's key says "This node is someone else's
home", with no button: a phone writes and clears only its own person's
key, and another's goes only by a removal (Phones at the node, The
home's owner).

**The home's owner.** The app knows the person is the owner from the
summary's flag, at each fetch, and at the home from Phones' flags; it
keeps the last word in `me.json`. The owner's commands ride on a fetch
from the node the phone is on, which needs a place, as the answer comes
back to it with the mail ("Connect to a node first" otherwise); each
says "Sent to your home; its answer comes with your mail", and the
answer is told once, as "No new mail" is. Away from the home, Settings'
Home node lists the people with a home there for the owner: **Show
them** sends the list command, and the rows come with the answer, in
the form of the node's phones (Phones at the node), "you" on the
person's own, with **Choices** on another's: **Remove**, "Remove Bo's
home?", "Your home stops keeping their mail.", which says "Removed from
your home" or "Already gone", and **Make owner**, "Make Bo the owner?",
"The owner chooses who has a home here. You no longer can.", which says
"Bo is your home's owner now". A command from someone who is no longer
the owner says "Your home says you are not its owner". At the home the
same is done by the code on its screen, under This node. A command
retried after its answer was lost does no harm (spec "Mailbox").

**A home given.** The owner's **Share my home node**, in a contact's
thread menu, takes one tap: the app makes a random 32-byte mailbox key
for the contact, works out their recipient tag at the home from their
card, and sends the add command with the key, the tag and the card's
name. On "ok" or "already there" it sends the contact one message, a
record of the home's address and the key (spec "Phone link", The
payload), and says "Ann has a home at yours now. They are told"; "full"
says "Your home is full: it keeps eight people". The key is kept with
the contact for that home (`contacts.json`, `share`), so a tap after a
lost answer sends the same key, which the home holds already, and the
contact is sent it again. The contact's app keeps the home given with
the contact (`gift`) and shows it on the first screen and in the
thread; **Use it** makes it the home with the given key (`me.json`,
`homeKey`; leaving the home before, above), changes the card, which goes
to every contact, and fetches from the node the phone is on, or the
next one linked, which the summary answers; nothing goes back to the
giver. "ID … is your home" says it.

**Using an existing home.** A phone that has the person's identity from
their 12 words but not their home has **Use my existing home** under
Home node. Its dialog, "Your home's address", takes the 26 hex digits
of the home's address, as an old card's Details show it, or a card of
the person's that names it, pasted or scanned with **Scan a card**,
saying which node it read, then **Ask it**: the app fetches from that
address with the key the identity makes, from the node the phone is on,
and says "Asking ID … for your mail". A summary from it makes it the
home, "ID … is your home", whose card then goes out as above; with
none within the fetch's wait, the line says "ID … did not answer.
Nothing changed." and the button comes back.

**A home removed.** A person whose home was removed, at the node or by
its owner, learns it in one of two ways. At the home, Home says it is
not theirs and no other phone of theirs keeps it (above), as the phone
links or while it is linked: the app keeps
the home, as the person is there to set it again, a notice says "Your
home no longer keeps your mail. Set it again in Settings.", and Settings
shows **Set home** on the home, with the line "Your home
no longer keeps your mail". Away from it, the node answers their next
fetch, with a command or not, by a summary that says so and nothing
else (spec "Mailbox", The answer). To a fetch to the home, the app then has no
home, as after **Remove home**, sends the card without it to every
contact, and tells "Your home no longer keeps your mail. Choose a new
home in Settings."; to the old home while leaving it, the old home has
forgotten the person already, and no leave goes; to a home named under
**Use my existing home**, it tells "ID … does not keep your mail.
Nothing changed." A home that does not answer at all, as one that lost
its memory or gave the person's place to another, is not told from one
that is off or out of reach: once the last fetch to the home had no
answer within its wait, Home node says "Your home did not answer your
last fetch.", and its help says to fetch again later and, if it still
does not answer, to make another node the home or choose **Remove home**. Nothing changes by itself.


## Where a text goes

A message names the node its sender writes from, while the other side
may not know it, and a card names the node its person's phone uses, with
when it was last linked to it, so the app knows two addresses for a
contact: the home on their card, if they have one, and one place they
were last known at, from whichever of their messages and cards is
newest: a message counted from the time it was written, or from when
it arrived if that is earlier, a card from its link time; a message
without an address keeps the place and moves its time. While that
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
ago" and still sends there. With a home and no place known yet, it goes to the home (roadmap
D132, D162). Reactions and
cards go without asking, to the home's mailbox when there is a choice.

## The identity file

A person moves to a new phone with an identity file (Settings, My
identity; `lib/identity_file.dart`). The 12 words bring back the
identity alone; the file brings back what goes on from it: the seed,
the name, the home's address and the mailbox key given to it, the
card's times, the last `sent` given, so that the new phone's come after
it, as a contact takes only a newer one as news and the home answers
only a fetch later than its last (spec "Mailbox"), the mailbox's latest
rounds, every contact with its card, name, verified mark, last known
place and archived mark, the nodes the old phone was paired with and
where it stood in each public node's ring as a guest, and, if the person
ticks **Add my messages**, the queue, the received texts and the
reactions used. Whether the app asked for Bluetooth and to notify is
the phone's, and stays.

Android keeps the app's files out of its backups and out of a transfer
to a new device (`android:allowBackup="false"` and
`res/xml/data_extraction_rules.xml`), as they hold the seed and the
mailbox key: the words and this file are the only ways to move.

The file is a header of 54 bytes, the ASCII `bark-identity`, the
layout's version, 1, a salt of 16 random bytes and a nonce of 24, then
the contents as JSON, encrypted and authenticated with
XChaCha20-Poly1305 (`crypto_aead_xchacha20poly1305_ietf`), the header as
its additional data, so that a byte changed anywhere fails the open. The
key is Argon2id (`crypto_pwhash`, `argon2id13`) of the password and the
salt with RFC 9106 §4's second recommended option, for where its first
one's 2 GiB is more memory than there is: 3 passes over 64 MiB, a
128-bit salt and a 256-bit key. libsodium runs it in one lane, not that
option's four, so it takes longer, in an isolate of its own, under a
spinner; the memory, which is what each guess at the password costs, is the
same. The contacts and the messages are in the forms of
`contacts.json` and `bark.json`, each with that file's version, and the
app reads only a file whose versions are its own, the messages also of
`bark.json`'s version 3, as its messages had the same forms: another is
"from another version of the app". The cipher cannot tell a wrong password
from a damaged file, and says both. The home carries its key as a
field of its own, the one the identity makes or one given with the home
(A home given, above), which the new phone keeps as given unless it is
the identity's own. Whether the person is the home's owner is not in the
file: the next summary says it.

A restore reads the whole file before it changes anything, ends the
link, which was made for the identity replaced, and replaces the
person, the contacts and the messages, none if the file has none. A
pairing is the phone's: Android keeps the bond, and the node knows the
phone by it (Old bonds, above), so the nodes the old phone was paired
with come back paired but not this phone's, until the person pairs
again: they come back as nodes that do not know the phone, "Does not
know this phone" under "Paired with", with **Pair again** while in
reach and **Forget** (Settings, above), and no mark of their own, as
that is all they are to this phone. The search does not
use them meanwhile, and a public one serves the phone as a guest. A node
this phone is paired with already stays its own; and where the file
comes back to the phone it was saved on, the app's files gone, Android
holds the bonds still, so a node whose bond Android lists when a scan
first hears it is the phone's own again, without a code. The home needs no
pairing, as it holds the mailbox key: texts to the card go on arriving
there, and **Fetch my mail** from any other node brings them. Each
saved file holds the state of its moment: two phones restored from one,
or the old one used on, would each take mail the other never sees, so
the Save dialog says "Use it on one phone at a time."

## Plugins

Besides those for Bluetooth, location and storage, pinned packages
handle the QR code, the cryptography, the notifications, the map and
the emoji list, all under
licences compatible with the repository's GPL-3:

| package | does | licence |
|---|---|---|
| `sodium` 4.1.2+1 | libsodium 1.0.22, built from its source into the app, and its Dart binding; the identity file's Argon2id is in its "sumo" part, which on Android is the same library | BSD-3-Clause; libsodium ISC |
| `crypto` 3.0.8 | SHA-256, for the backup words' checksum and a firmware image's hash | BSD-3-Clause |
| `file_selector` 1.1.0 | picks a firmware image file and an identity file to restore | BSD-3-Clause |
| `qr_flutter` 4.1.0, with `qr` | draws the code | BSD-3-Clause |
| `qr_code_scanner_plus` 2.3.2 | scans it with the camera | BSD-2-Clause; on Android it uses the ZXing libraries, Apache-2.0 |
| `flutter_local_notifications` 22.3.3 | the texts' notifications and the permission to notify | BSD-3-Clause |
| `emojis` 3.2.0 | every emoji of Unicode 17.0 with its name, keywords and group, for the reaction picker | BSD-3-Clause |
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
from the repository's secrets (`DEVELOPING.md`, "Releases").

The tests check every value's bytes against the vectors of
`bark-link/src/tests.rs`, the message ID among them; check keys,
words, cards and every payload form against `bark-app-layer`'s
`vectors.json`, made by the Rust side; run the queue, the inbox, first
messages, reactions of one character and no more, the stored files
and cards in their current forms alone, an identity file's key against
a vector of Argon2id, the file saved and restored on another phone with
its messages and without, its home's key, a given one kept as given,
and the nodes to pair again,
and refused, changing nothing, for a wrong password, a changed byte,
another file and another layout, a release read only with its
manifest, a newer app's APK kept only once whole and handed to the
installer, the picker's first
row after picks and a restart, a reflected
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
the home only where a home of the person's tag is listed, tell the person where someone
removed it, and refuse a node that does not keep it; leave an old home
by fetching its mail and then sending the leave, sent once and the home
forgotten when its answer is lost; seal and open the owner's commands
and the summary's flag, outcome and list against the Rust vectors, and
a home given as a record of its own; share the home with a contact, the
key reused after a lost answer, list, remove and hand over from away
and be refused after, use a home given with its key, name an existing
home that answers or does not, take a home that says it was removed,
fetched from, left or named, and a home that stays silent, and take an
empty owner's seat at the home (`test/owner_test.dart`, `test/owner_screens_test.dart`); make the fake
public, join it as a guest that writes its tag and touches nothing
closed, read a guest's ring items by its tag after the number it keeps
for the node, across links and restarts, close a second guest, end a
guest's link after 30 s, and take turns as Ready, given only to a phone
that asks, two idle phones seeing no flips;
decide what the search does in tables of situations, the combinations
met on the bench among them (`test/decision_test.dart`); choose the home
among the paired nodes, else the strongest paired
node, else the strongest public node; go on a guest's errand (connect,
tag, read, send, leave) when something waits to be sent and otherwise
once a minute, try again after being closed for another guest, and move
to a paired node once it is heard; with paired nodes, go to a public
node only after two rounds without any, send through it as a guest,
name it on the card and on the first screen's line, come back to a
paired node in the round that hears it, and stay with a paired node
heard every other round; say each of the four lines, with the home and
without, and why in the line's help; keep every node paired with, each
lost apart, across restarts; list them in Settings, the home first,
connect to
one on a tap, after a scan when it was not heard, and say "Not looking.
Tap to connect" with the search stopped, Disconnect below the list;
use a node of a newer link version that still serves the app, as a
paired phone and as a guest, with the quiet line, and close one that does not,
by its Node or by its answer to Recipient, or one older than the app
serves, saying which side to update, and pass it over until a tap or a
link to it; write the name and the versions
to Recipient; list a node's
phones in two lists with their names and days, remove another's with
the code its screen shows, and offer a new code after a wrong one, and
make one the owner by it; say
that a full node is full, and try no pairing there; make a
guest's node the home by pairing first, offered on the first screen
and in Settings, and stay a guest after a wrong code; from the home,
take a tapped public node as a guest with no round between and keep it
until the node ends it, and pair with one by its Pair at once; have
Android remove its bond with a node the app does not count as paired as
it hears the node, before any link, by the search or a guest's tap,
and use the node as a public one, and where Android keeps the bond,
pass the node over, saying so once, until the bond is forgotten; on unpairing, have Android forget its bond too,
also when the link ends under the write, say what to do where it keeps
it, and forget nothing when the write fails with the link up; read the fake
node's neighbours, in one page and across several; group texts into
conversations; split texts at boundaries as the Rust vectors do;
choose where a text goes; decide what notifies, and when the app starts
with the phone, against a notifier that writes down what it is asked;
and drive the screens: Settings' sections in their order, a text to
the fake friend, its echo, a
reaction each way, from the first row, the full list and a search,
on the bubble's lower edge, the backup words, a contact from a pasted or a scanned
card, my card, the debug screen, the fetch button, the choice of where
a text goes, a long text's parts, the bars for a newer release against
a fake fetch, and the map, its markers, lines,
legend and a neighbour's details, nodes at one place under one
marker that lists them by ID and kind, the people at a node under its
ID, over tiles that cannot be had. The Android side, the service and
the boot receiver, cannot run on the host; it is kept small and tried on
a phone (Trying it).

## Trying it

1. Switch the node on and read the twelve hex digits on its display.
2. In **Settings**, press **Scan** and allow
   Bluetooth. The node shows within about a second as `ID` and those
   digits, as its screen shows them. Tap it: the phone uses it as a
   guest, and Settings says so. Under the home node, **Set home**, whose dialog says the phone pairs first, or **Pair** on its
   row in the scan. Android asks for a code and the node's screen shows
   `PAIR CODE` and six digits within a few seconds: type them. A wrong
   code says "Not paired", and the phone goes on as a guest. Settings
   then says "Connected to ID …, paired" (or "Connected to your home,
   ID …") and how the node fares: "Starting up, m:ss" in its first rotation, "Hears N nodes",
   sleepy ones counted, or "Hears no other node yet: texts wait here";
   and
   the node's screen, `PHONE UNPAIRED` before, `PHONE PUBLIC`.
3. **Debug** shows link version 1, the node's clock (from its GPS if it
   has a fix, else from the phone, with the phone's offset) and, on a node
   fresh from boot, the
   first rotation counting down from 3:15 (a relay) or 6:30 (a sleepy
   node), the relays it knows and the neighbours it hears, sleepy ones
   too.
4. A node with a GPS fix or a map point shows "Placed: yes" and an
   address, and **Nearby nodes** draws it with the relays it hears, each line's
   colour its link's quality. Without one, Settings under **Home node**
   says "This node has no place yet"; set one under **This node**,
   **Place**, typed or by **Use this phone's location**: Status changes
   to placed and map point. A text names the node's address only once it
   has one. With neighbours, each marker
   shows its screen's role within a second or, asked again, a few;
   the node's log says `ble: ask` for each ask, `status of` for each
   answer, and `StatusGivenUp` for an ask or answer given up on a busy
   channel, which tells it from one lost on the air. A sleepy node with
   an anchor shares the anchor's marker, which shows a "2"; a tap lists
   both, and a tap on the sleepy one opens its details.
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
   too. Press and hold the answer and pick a reaction, one from the
   list below the first row: it shows on the answer's lower edge, and
   the other phone shows it on its text when it arrives.
6. Switch off the receiving phone's Bluetooth, send two texts to it,
   switch it back on and connect: both arrive, oldest first, and the
   inbox count goes back to 0.
7. With the receiving phone connected, its screen off for ten minutes,
   send it a text: it sounds and shows the text under the sender's
   name. A second text joins the same notification. Tapping it opens
   the thread, and the notification goes.
8. Given back and not ready happen only when the network refuses: a
   text given back busy counts down its wait and goes again by itself,
   and one answered not ready stays queued until the node takes it. A
   text to an address no node owns, as a contact whose node is off,
   comes back "Found no way there" once the search is done; **Send
   again** sends it as a new message, whose ID Debug shows, and
   **Cancel** removes it.
9. On a node on USB that stays where it is, **Settings**, **Make this
   node my home**: Debug's Home reads a home, and the node's log shows
   `ble: home set`. Texts to your card now go to it and wait in its
   mailbox. Connected to another node, **Fetch my mail** on the first
   screen says "Fetching your mail" and brings them there, or "No new
   mail"; the home's log shows `mailbox: fetch … taken` and a
   `mailbox: reply … summary` line and a `mailbox: reply … item` line
   for each item.
10. Two phones on one node, its log open. With the
    first phone paired and its name set, pair the second with the same
    node, its name set too: it asks for the code, the node's screen shows
    a new one, and once typed the screen says `2 PHONES PUBLIC`; the
    first phone stays connected and stays paired. On each phone,
    **Settings**, **This node** lists both names under "Phones paired
    with this node", its own marked "This phone, linked now" and
    the other "Linked now"; on the first, open all along, the second's
    row comes as its pairing ends, with no new connection. Make the node the first phone's home: the
    first phone's row shows under "Phones with a home here" too, on both
    phones at once, the second linked all along. On the first
    phone, **Remove** on the second's row under "Phones paired with this
    node", **Show the code**: the node's screen shows `CONFIRM` and six
    digits. Type a wrong code: "Not removed", and the log shows `ble:
    removal refused`; **New code**, type the one shown: "Removed", the
    log shows `ble: record … removed`, the second phone's link ends, and
    the screen says `PHONE PUBLIC`. The second phone, connecting again,
    says "This node is no longer yours", with **Pair again**, which takes
    the code on the screen. With eight phones paired, a ninth's pairing
    shows no code, the screen says `PHONES FULL`, and the ninth says "Not
    paired", naming a full node among the reasons; as a guest there it
    says "This node is full".
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
13. An update from the phone (Updating a node), on a node with the
    loader, version N, on USB with its log open. On
    the host, from `bark-fw/`, build an image one version up, for the
    bench only, and copy it to the phone:
    `BARK_BENCH_ONLY=1 BARK_IMAGE_VERSION=N+1 cargo build --release &&
    scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw
    target/vN+1.img`, then sign it with the bench key, which a
    bench-only build holds: `scripts/image.py sign target/vN+1.img keys/bench.pem
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
    bar "App version A is out", tapped, says "Downloading…" and opens
    Android's installer for the APK, which refuses it over this debug
    build, signed with another key (Building), and its ×
    hides it, also after the app is closed and opened again. Connected
    to the node, the bar "Node firmware F is out" opens **Update firmware** with
    "Version F" chosen, and its × hides it for that node; another node
    of version 1 still shows it. A node of version 0 shows none.
15. A public node, with two phones. On the paired
    phone, connected, **Settings**, **This node**, **Public** is on from
    the start, and the node's screen says `PHONE
    PUBLIC`; turned off, it says `PHONE PRIVATE`, and on again `PHONE
    PUBLIC`. On a second phone that has never paired with it, **Scan**
    lists the node as public with its signal; with no paired node of
    its own in reach, the second phone goes on an errand there within a
    round, without asking anything, its first screen saying "Connected
    to ID … as a guest", with an errand once a minute,
    and, with no home set, offering **Set home**; **Not
    now** hides it until the app is started again. No pairing dialog
    shows, at the connect or later. Its log shows `Recipient: tag …`, a
    `Guest: item …` line for each item read, and `Disconnected`.
    A text between the two phones' contacts reaches each, and neither
    sees the other's; a text sent to the second phone while it is away
    from the node is read at its next errand there. Taken near its own
    paired node, the guest phone moves to it within a round. The paired
    phone, its node switched off or carried away, says "Connected to ID …
    as a guest. Home at ID …" within about 30 s if another public node is
    in reach, its info button saying that none of its paired nodes has
    been in reach for 30 s; it sends a text through it, and says
    "Connected to your home, ID …" within a round of the node's return.
16. Unpairing. On the paired phone, connected, **Settings**,
    **Unpair** on this phone's row of the node's phones, **Unpair**:
    the node's screen says `PHONE UNPAIRED`, Settings says "Unpaired. The
    node is public.", and the phone's Bluetooth settings no longer list the
    node; the log shows `Pairing: the phone forgot its bond with …`.
    Within a round or two the phone goes on an errand at the node as a
    guest. On a phone that keeps the bond, a dialog says to forget the
    node in the phone's Bluetooth settings, and **Bluetooth settings**
    opens them; the first screen says "The phone kept its old pairing
    with ID …", and the line goes within a round of forgetting it there,
    after which the phone uses the node as a guest.
17. Two paired nodes, A and B. Make A the home, then pair with B in
    Settings (**Pair** on its row in the scan if public, a tap on it if
    private). Settings lists both under "Paired
    with", A first as "Your home". Switch B off and take the phone to A:
    within a round the first screen says "Connected to your home, ID …",
    A's ID, never "as a guest". With A switched off and B on, it says
    "Connected to ID …, paired. Home at ID …", B's ID then A's, within a
    round, without the 30 s wait. **Disconnect**, below the list: both rows say "Not looking. Tap to
    connect", and a tap on A's connects to it, within a scan when it was
    not heard in the last 30 s.
18. A tap on a public node from the home. On the home, with a public
    node C in reach that the phone is not paired with, **Settings**,
    **Scan**, a tap on C's row: the line says "Connected to ID … as a
    guest for 30 s", C's ID, the
    node logs the phone's Recipient, and the log has no `Found …,
    connecting` until C ends the link about 30 s after it began; then
    the phone is back on the home within a round. **Pair** on C's row,
    **Pair**: Android asks for the code on C's screen, and Settings
    lists C under "Paired with"; the phone stays on C, as the person
    chose it, until its link drops.
19. A node that forgot the phone while Android still holds the bond.
    Pair the phone with a node, then have the node forget it: a second
    phone paired with the node removes the first (Phones at the node),
    or the node's stored state is discarded. Connect the first phone
    once, which finds the node no longer its own, and **Not now**.
    **Settings** lists the node under
    "Paired with" as "Does not know this phone", with **Pair again** and
    **Forget**;
    a tap on the row says what happened. **Pair again**: the log shows
    `Pairing: the phone forgot its bond with …`, the node logs one
    link, shows a code, and the pairing completes; the row says the node
    is the phone's again. **Pair** on its row in the scan does the same.
    A pairing that fails, a code typed wrong or cancelled, or a node
    switched off before the code, ends in a dialog that says why.
20. A node of a later link version that does not serve the app (roadmap
    D208). Two paired nodes, A the home and B, and a
    public node C. Flash A with a build whose `bark-link` `VERSION` and
    `OLDEST` are 2, not committed. With A and B in reach the first
    screen says "ID … needs a newer app. Update the app.", A's ID, and
    within a round the phone is on B, as a paired phone; the log has one
    `Found …, connecting` for A and no more, and no scan while on B.
    **Settings** says of A "Your home, cannot be used yet. Tap to
    try again". Dismiss the notice: it does not come back. A tap on A's row
    tries once, and the notice comes back. Switch B off: after two rounds,
    30 s, the phone is on C as a guest. Restart the app: A is
    tried once again. Flash A back to version 1 and tap its row: the
    phone links to it, and the search takes it again.

21. Moving to a new phone, with the Pixel as the new one. On the old
    phone, with a home, contacts and a node paired, **Settings**, **My
    identity**, **Save to a file**: type a password twice, tick **Add
    my messages**, **Save**, and save the file in Android's dialog, to
    Downloads or Drive. On the Pixel, **Restore from a file**, **Choose
    file**, the file; a wrong password says so and changes nothing; the
    right one says "Identity restored. Pair again with your nodes.", and
    the conversations, contacts, name and card are the old phone's.
    **Settings** lists the old nodes as "Does not know this phone"; a
    text to the restored person's card arrives at the home, and **Fetch
    my mail** from another node brings it. **Pair again** on the home's
    row, with the code on its screen: the row says "Your home".
22. The owner, at the node. Phone P makes node A its home (step 9):
    **Settings**, **This node**, its row under "Phones with a home
    here" says "This phone, owner, linked now". Phone Q pairs with A and
    makes it its home too: on Q, its own row has no "owner", and P's
    has it. On Q, **Choices** on its own home row, **Make owner**,
    **Show the code**: A's screen shows `CONFIRM` and six digits; typed,
    with **Make owner**, it says "Q is the owner", Q being the name on
    Q's card, and on P the "owner" mark moves to Q's row within a moment.
    On Q, **Choices** on P's home row, **Remove**, the code: P's row
    leaves "Phones with a home here", and P, at its next link to A, is
    told "Your home no longer keeps your mail"; away from A instead, P's
    next **Fetch my mail** brings "Your home no longer keeps your mail.
    Choose a new home in Settings.", **Settings** says "None: your card
    is temporary.", and P's card goes to its contacts. With the owner's home
    removed, "This node has no owner" shows on both; on the phone whose
    home it still is, **Become owner** says "You are its owner".
23. A home given, with two nodes, A, P's home, of which P is the owner,
    and B, near Q, and Q in P's contacts. On P, connected to any node
    with a place, Q's thread, the menu, **Share my home node**: "Sent to
    your home; its answer comes with your mail", and A's log shows a
    fetch taken and a summary; then "Q has a home at yours now. They are
    told", and a message goes to Q. On Q, the first screen and Q's
    thread with P say "P gave you a home at ID …", A's ID. **Use it**:
    "ID … is your home", Q's home under **Settings** is A's address,
    Debug's card address A's, a fetch goes to A from B and is answered,
    and **Send my card** is not asked for: the card went to Q's
    contacts. A second **Share my home node** on P sends the same key:
    A's log shows the add answered "already there", and Q is sent the
    home again.
24. The owner away. On P, connected to B, not A: **Settings**, Home
    node, "People with a home here", **Show them**: within the time the
    mesh takes, the rows of A's people come, P's as "You, owner".
    **Choices** on Q's row, **Remove**, **Remove**: "Sent to your home…",
    then "Removed from your home"; at A, Q's row in Phones on a phone
    there no longer says home, and Q, away from A, is told at its next
    **Fetch my mail** as P is in step 22. Share with Q again, then **Make owner** on
    Q's row: "Q is your home's owner now"; P's "People with a home here"
    goes, as P is no longer the owner, and a share from P now says "Your
    home says you are not its owner".
25. Leaving. On Q, whose home is A, connected to B: **Settings**, **Remove
    home**, **Remove**: "Home removed. Your old home sends you its
    mail, then forgets you.", and the line "Leaving ID …" shows. Q's log
    says `Leaving: a fetch to bark-…`, A's log a fetch taken and its
    items, then `Leaving: the leave to bark-…`, A's log a fetch with the
    leave, and `Leaving: bark-… forgot the person`; the line goes. At A,
    Q's record is gone. Making B the home while A is the home does the
    same to A.
26. An existing home after the 12 words. On a phone restored from P's
    12 words alone (**Restore from words**), connected to B: **Settings**,
    **Use my existing home**, paste the 26 hex digits of A's address (as
    P's old card's Details show it) or P's card: the field names A's ID;
    **Ask it**: "Asking ID … for your mail", and within the mesh's time
    "ID … is your home". With A switched off, the line says, after 32
    minutes, "ID … did not answer. Nothing changed."
27. At the home as a guest. The phone of step 26, at A, which it is not
    paired with: it goes on its errands at A as a guest, and a text to P's
    card that arrived at A is read at the next errand, though A's record
    for P claims it; **Fetch my mail** says "At your home, your mail comes
    by itself".
28. Home's start. On a phone whose home is A, at A: **Remove home**,
    then **Set home** again: the node's log shows `ble:
    home of record … set`; a **Fetch my mail** from B right after is
    answered as usual.
29. GPS signal. **Nearby nodes**, a tap on this node: the GPS signal row
    says the node's number, such as `38 dB-Hz · enough for a fix` outdoors; indoors,
    away from a window, it falls under 22 to `too weak for a fix`. A
    relay that has stood still a day rests its GPS, and the row then adds
    how long ago its last reading was, such as `, 40 min ago`; a sleepy
    node with an anchor that has had no reading since it started says
    `no reading`. The refresh button reads it again. A neighbour's row comes with its answer. Debug's
    Status shows the same number.
30. Show where I am. **Nearby nodes**: no permission is asked as it
    opens. **Show where I am**: Android asks for location while the app
    is in use (the first time only); a blue dot shows where the phone is.
    On a sleepy node with an anchor, the node's marker is at the dot,
    with a thin line to its anchor, and its details' Place says
    `its anchor's`; on a relay with a GPS fix, its marker stays at its
    own place, apart from the dot. Denied, it says "Location permission
    denied" and the map is as before.

## The phone test

`integration_test/phone_test.dart` runs the app on a phone against a
board, with the app's own files, so its identity and pairings, and says
each of five steps in the Android log as passed, failed or skipped, with
why; `bark-fw/scripts/phonetest.py` builds it, runs it and reads it
(`docs/bench-suite-2026-11-13.md`, row A7). The search runs from the
start, as in `main.dart`. A step goes through the screen where the
screen itself could break, and through the app's model where a tap only
calls it; one whose earlier step failed is skipped.

1. **find**: Settings, **Scan**, until the node named for the test is
   listed.
2. **pair**: a tap on its row, over the bond the phone holds; or its
   **Pair** and the dialog's, and the code in Android's dialog; until the
   phone is linked as a paired phone. Asked for a fresh pairing, the phone
   first unpairs from the node, or forgets it while not linked.
3. **send**: the second node's address from the node's neighbour table;
   a contact, **Bench friend**, made by the app's own code from a fixed
   identity, whose home it is; a text typed in its thread, until the node
   has taken it. The network says nothing of arrival (Screens), so the
   script looks for the text's message ID in the second node's log.
4. **receive**: Bench friend's first message to the person, sealed by the
   app's code to the person's card, which the script has a board send to
   the node; until it is read and opened.
5. **update**: the image the script put in the app's files, sent from the
   update screen and rebooted into, until the node is back on its
   version.

A bench-only build writes the pairing's code to its log
(spec "Phone link", Pairing): the script reads it from the board's log
once step 1 has passed and types it into Android's dialog with
`adb shell input`. Another build's code is typed by hand. The steps also run against the
fake node, in `test/phone_steps_test.dart`.

```
cd bark-fw
scripts/phonetest.py --node <A> --to <B> --node-log <A's log> \
    --to-log <B's log> --image target/vN+1.img \
    --send 'python3 scripts/bench.py say {to} "payload {dst} {payload}"'
```

The logs are as `scripts/bench.py capture` writes them. `--send` is the
command by which a board sends a payload to an address, with `{dst}`,
`{dst_id}`, `{payload}`, `{id}`, `{node}` and `{to}` in it; the one above
has B, a bench build, send it with its `payload` command
(`bark-fw/README.md`, Bench build). Without it the script prints what to
send. `--pair` is `any`, the default, `bond` or `fresh`. The
script installs the test's build over the app, keeping its files: it
never uninstalls, so over an app signed with another key the install
fails. It grants the Bluetooth, location and notification permissions,
copies the image into the app's files by `run-as` and wakes the screen,
which must have no lock. Node A needs a place, and B must be among its
neighbours, with a place. Exit status 0 is every step passed, 1 a step
failed, 2 a run that did not end. Afterwards the phone runs the test's
build: build and install the app again, as a restart of the phone would
start the test (Starting with the phone).
