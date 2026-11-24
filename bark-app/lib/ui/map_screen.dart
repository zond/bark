// The paired node and the neighbours it knows on a street map, a line to
// each coloured by the link's quality (spec "Phone link", Neighbours).
// Read when the screen opens, at every status the node gives and when
// the person asks. When it opens, when the person asks and when they
// open a neighbour's sheet, each neighbour without a fresh answer is also
// asked what its own screen shows, and asked again while it does not
// answer (spec "Phone link", Ask; [Asks]), and its marker says it in
// those words. Nodes too close on the screen for a finger to choose share
// one marker, which lists them.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_map/flutter_map.dart';
import 'package:intl/intl.dart' show NumberFormat;
import 'package:latlong2/latlong.dart';

import '../app.dart';
import '../asks.dart';
import '../l10n.dart';
import '../link.dart';
import '../session.dart';
import '../phone_place.dart';
import 'common.dart';
import 'fact_help.dart';

/// OpenStreetMap's tiles, as its tile usage policy asks to be named.
const osmTiles = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';

/// The deepest zoom the OpenStreetMap tile server draws.
const osmMaxZoom = 19;

/// How long a tile is used without asking the server again. The map is
/// for a network that runs without the internet, so a tile once seen is
/// kept for a long outage; streets change little in a year.
const tileFreshFor = Duration(days: 365);

/// The tiles from the network, kept on the phone.
TileProvider cachedTiles() => NetworkTileProvider(
  cachingProvider: BuiltInMapCachingProvider.getOrCreateInstance(
    overrideFreshAge: tileFreshFor,
  ),
);

/// A link's colour, by its quality.
Color qualityColor(Quality q) => switch (q) {
  Quality.good => Colors.green.shade700,
  Quality.fair => Colors.amber.shade700,
  Quality.fragile => Colors.red.shade700,
};

/// A link's quality in a word, as a row's value says it.
String qualityName(Quality q) => switch (q) {
  Quality.good => tr.factLinkGood,
  Quality.fair => tr.factLinkFair,
  Quality.fragile => tr.factLinkFragile,
};

/// A link's quality as a screen reader names its row's icon.
String _qualitySaid(Quality q) => switch (q) {
  Quality.good => tr.factLinkGoodName,
  Quality.fair => tr.factLinkFairName,
  Quality.fragile => tr.factLinkFragileName,
};

bool _placed(Position p) => p.lat != 0 || p.lon != 0;

/// A signed number with the minus sign typography uses.
String _signed(num v, {bool plus = false}) {
  final r = v.round();
  return r < 0 ? '\u2212${-r}' : (plus ? '+$r' : '$r');
}

/// `v` to one decimal, in the language spoken's form, ungrouped.
String _tenths(double v) => NumberFormat('0.0', tr.localeName).format(v);

/// A distance in metres, in kilometres from 1 km.
String distance(double m) =>
    m < 1000 ? tr.factMetres(m.round()) : tr.factKilometres(_tenths(m / 1000));

/// Seconds since, as a person says it.
String secondsAgo(int s) =>
    s < 60 ? tr.timeSecondsAgo(s) : ago(Duration(seconds: s));

/// A position to five decimals, about a metre.
String _where(Position p) =>
    '${p.latDeg.toStringAsFixed(5)}, ${p.lonDeg.toStringAsFixed(5)}';

/// The neighbours a node knows as its screen's KNOWN line counts them:
/// heard, heard with the margin a full frame needs through a fade, and
/// those that have taken a message from it (spec "Neighbour status").
String known(int heard, int strong, int exchanged) =>
    tr.factKnownValue(heard, strong, exchanged);

/// [known] of the table read.
String knownOf(NeighbourTable t) => known(
  t.list.length,
  t.list.where((n) => n.quality(t.floorDbm) != Quality.fragile).length,
  t.list.where((n) => n.proven).length,
);

/// One fact of a sheet: an icon named for screen readers and a short
/// label, which a newcomer reads where an icon alone would leave them
/// guessing, and its value.
class Fact {
  final IconData icon;

  /// Which fact it is, whose label and help it shows.
  final FactKind kind;
  final String name;
  final String value;
  final Color? color;

  /// Said on a long press, for a value whose parts need naming.
  final String? tooltip;

  /// This node's own figures, shown under the row's help.
  final String? detail;
  const Fact(
    this.icon,
    this.kind,
    this.name,
    this.value, {
    this.color,
    this.tooltip,
    this.detail,
  });

  String get label => factLabel(kind);
}

/// The facts of a node's power: USB, or its battery, which is low, read
/// as `reading`, or not read.
Fact _power({required bool usb, bool low = false, String? reading}) => usb
    ? Fact(Icons.power, FactKind.power, tr.factPowerName, 'USB')
    : Fact(
        low
            ? Icons.battery_alert
            : reading == null
            ? Icons.battery_unknown
            : Icons.battery_std,
        FactKind.power,
        tr.factBatteryName,
        low ? tr.factBatteryLow : reading ?? tr.notRead,
      );

/// The GPS's signal: its number and, by [gpsFixFloorDbHz], whether a fix
/// is taken at it; 0 is no reading (spec "Neighbour status").
Fact gpsFact(int cn0) => Fact(
  Icons.signal_cellular_alt,
  FactKind.gpsSignal,
  tr.gpsSignal,
  cn0 == 0
      ? tr.noReading
      : cn0 < gpsFixFloorDbHz
      ? tr.factGpsTooWeak(cn0)
      : tr.factGpsEnough(cn0),
);

/// What a node does for others: a relay passes texts on; a sleepy node
/// sleeps between windows and sends and receives through its anchor
/// (spec "Nodes").
Fact _role(bool sleepy) => sleepy
    ? Fact(
        Icons.bedtime,
        FactKind.role,
        tr.factRoleSleepyName,
        tr.factRoleSleepy,
      )
    : Fact(
        Icons.cell_tower,
        FactKind.role,
        tr.factRoleRelayName,
        tr.factRoleRelay,
      );

/// Whether a node is carried (spec "Nodes").
Fact _moving(bool carried) => carried
    ? Fact(
        Icons.directions_walk,
        FactKind.moving,
        tr.factMovingName,
        tr.factCarried,
      )
    : Fact(Icons.house, FactKind.moving, tr.factMovingName, tr.factFixed);

/// A sleepy node's anchor: `to` names it when known, as for this node's
/// own, whose anchor is the neighbour the table marks; a neighbour's
/// anchor the link does not name (spec "Phone link", Neighbours).
Fact _anchored(bool anchored, {String? to}) => anchored
    ? Fact(
        Icons.anchor,
        FactKind.anchor,
        tr.factAnchorName,
        to == null ? tr.factAnchored : tr.factAnchoredTo(to),
      )
    : Fact(Icons.link_off, FactKind.anchor, tr.factAnchorName, tr.factNoAnchor);

/// Where a node's position comes from, GPS or a map point a person set,
/// and the position on a second line; none without a place.
Fact _position(Position p, {required bool mapPoint}) => !_placed(p)
    ? Fact(Icons.location_off, FactKind.place, tr.factPlaceName, tr.factNoPlace)
    : mapPoint
    ? Fact(
        Icons.push_pin,
        FactKind.place,
        tr.factPlaceByPersonName,
        tr.factPlaceByPerson(_where(p)),
      )
    : Fact(
        Icons.satellite_alt,
        FactKind.place,
        tr.factPlaceFromGpsName,
        tr.factPlaceFromGps(_where(p)),
      );

/// The people at the node `id`: those whose home it is, "You" first when
/// it is the person's, and the contacts last seen there, with when. A node
/// on the map is often a person's.
({List<String> home, List<(String, DateTime)> seen}) peopleAt(
  AppModel app,
  List<int> id,
) {
  bool at(Address? a) => a != null && hex(a.id) == hex(id);
  return (
    home: [
      if (at(app.me.home)) tr.mapYou,
      for (final c in app.contacts.active)
        if (at(c.card.home)) c.name,
    ],
    seen: [
      for (final c in app.contacts.active)
        if (at(c.place) && c.placeAt != null) (c.name, c.placeAt!),
    ],
  );
}

/// The people at `id` in one line, as a list of the nodes at one place
/// says them under each node: no times.
String? peopleLine(AppModel app, List<int> id) {
  final p = peopleAt(app, id);
  final parts = [
    if (p.home.isNotEmpty) tr.mapHomeOf(p.home.join(', ')),
    if (p.seen.isNotEmpty) tr.mapLastSeen(p.seen.map((s) => s.$1).join(', ')),
  ];
  return parts.isEmpty ? null : parts.join(' · ');
}

/// The people at `id` as a node's details say them: a line for its homes
/// and one for those last seen there, each with when.
List<String> peopleLines(AppModel app, List<int> id) {
  final p = peopleAt(app, id);
  return [
    if (p.home.isNotEmpty) tr.mapHomeOf(p.home.join(', ')),
    if (p.seen.isNotEmpty)
      tr.mapLastSeen(
        p.seen
            .map((s) => tr.mapSeenAgo(s.$1, ago(app.now().difference(s.$2))))
            .join(', '),
      ),
  ];
}

LatLng _at(Position p) => LatLng(p.latDeg, p.lonDeg);

/// The places a finger can tell apart among markers at `at` on the
/// screen: markers whose tap targets, squares of side `target` centred on
/// each, overlap share a place, and so on along any chain of them. Each
/// place lists its markers' indices in order, the places in the order of
/// their first.
List<List<int>> places(List<Offset> at, double target) {
  bool near(Offset a, Offset b) =>
      (a.dx - b.dx).abs() < target && (a.dy - b.dy).abs() < target;
  final taken = List.filled(at.length, false);
  final out = <List<int>>[];
  for (var i = 0; i < at.length; i++) {
    if (taken[i]) continue;
    taken[i] = true;
    final place = [i];
    for (var k = 0; k < place.length; k++) {
      for (var j = i + 1; j < at.length; j++) {
        if (!taken[j] && near(at[place[k]], at[j])) {
          taken[j] = true;
          place.add(j);
        }
      }
    }
    out.add(place..sort());
  }
  return out;
}

/// A node on the map: where its marker goes and how big it is, its name
/// and kind as a list of the nodes at one place says them, its marker,
/// with a count when it stands for several, and its sheet.
class _Spot {
  final LatLng point;
  final double width;
  final double height;
  final String name;

  /// The people at it, as [peopleLine] says them.
  final String? people;
  final String kind;
  final IconData icon;
  final Color color;
  final Widget Function(int count) look;
  final Widget Function() sheet;

  /// Done as its sheet opens.
  final VoidCallback? onOpen;
  const _Spot(
    this.point, {
    required this.width,
    required this.height,
    required this.name,
    required this.people,
    required this.kind,
    required this.icon,
    required this.color,
    required this.look,
    required this.sheet,
    this.onOpen,
  });
}

/// A marker's icon, with the number of nodes at its place when several
/// share it.
Widget _counted(Widget icon, int count) =>
    count < 2 ? icon : Badge.count(count: count, child: icon);

class MapScreen extends StatefulWidget {
  final AppModel app;

  /// Where the street map comes from; tests give one without the network.
  final TileProvider Function() tiles;
  const MapScreen(this.app, {this.tiles = cachedTiles, super.key});

  @override
  State<MapScreen> createState() => _MapScreenState();
}

class _MapScreenState extends State<MapScreen> {
  late final TileProvider _tiles = widget.tiles();
  StreamSubscription<Status>? _seen;

  /// Why the last read failed, if it did.
  Object? _error;

  /// A tile could not be had: no internet, and not kept on the phone.
  bool _noTiles = false;

  Session get _s => widget.app.session;

  /// The neighbours are to be asked for their status after the next read
  /// that succeeds: when the map opens, perhaps before the node is
  /// connected, and each time the person refreshes it. Not at every
  /// status, as each ask costs airtime.
  bool _ask = true;

  /// The link they were last asked on: the answers go with it, so a new
  /// connection asks again.
  Object? _askedOn;

  @override
  void initState() {
    super.initState();
    _seen = _s.statusSeen.listen((_) => _read());
    _readGps();
    _read();
  }

  @override
  void dispose() {
    _seen?.cancel();
    // Asked only while a person looks.
    _s.stopAsking();
    super.dispose();
  }

  bool get _readable => _s.connected && !_s.guest;

  Future<void> _read() async {
    if (!_readable) return;
    try {
      final t = await _s.readNeighbours();
      if (mounted) setState(() => _error = null);
      final link = _s.link;
      if ((_ask || link != _askedOn) && mounted) {
        _ask = false;
        _askedOn = link;
        await _s.askNeighbours(t.list);
      }
    } catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  Future<void> _refresh() {
    _ask = true;
    _readGps();
    if (_you != null) _showMe();
    return _read();
  }

  /// This node's GPS signal, read as the map opens and is refreshed, as
  /// the node does not notify it.
  void _readGps() {
    if (!_readable) return;
    _s.readGps().catchError((Object e) => _s.note('Status: $e'));
  }

  @override
  Widget build(BuildContext context) => Watch(widget.app, (context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(tr.nearbyNodes),
        actions: [
          IconButton(
            tooltip: tr.mapShowMe,
            icon: const Icon(Icons.person_pin_circle),
            onPressed: _showMe,
          ),
          IconButton(
            tooltip: tr.mapReadAgain,
            icon: const Icon(Icons.refresh),
            onPressed: _readable ? _refresh : null,
          ),
        ],
      ),
      body: SafeArea(top: false, child: _body(context)),
    );
  });

  Widget _say(String s) => Center(
    child: Padding(padding: const EdgeInsets.all(24), child: Text(s)),
  );

  Widget _body(BuildContext context) {
    final n = _s.node;
    if (n == null) {
      return _say(tr.mapNotConnected);
    }
    if (_s.guest) return _say(tr.mapGuest);
    final t = _s.neighbours;
    if (t == null) {
      return _error != null
          ? _say(tr.mapReadFailed(said(_error!)))
          : const Center(child: CircularProgressIndicator());
    }
    // The node's own position; without one, as a sleepy node's with an
    // anchor or one with no place yet, it is drawn where the phone is, as
    // the Bluetooth link puts it within metres of it.
    final own = _placed(t.own) ? _at(t.own) : null;
    final you = _you;
    // Without the phone's, at its address, where others reach it: a
    // sleepy node's is its anchor's.
    final st = _s.status;
    final drawn =
        own ?? you ?? (st != null && st.placed ? _at(st.address) : null);
    final anchor = t.list.where((n) => n.anchor).firstOrNull;
    // A neighbour with no place is no point of the map: it is drawn with
    // this node, which hears it, and has no line.
    final unplaced = t.list.where((m) => !_placed(m.position)).length;
    final points = [
      ?drawn,
      ?you,
      for (final m in t.list)
        if (_placed(m.position)) _at(m.position),
    ];
    if (points.isEmpty) {
      return _say(tr.mapNothing);
    }
    return Stack(
      children: [
        FlutterMap(
          options: MapOptions(
            initialCenter: points.first,
            initialZoom: 16,
            initialCameraFit: points.length < 2
                ? null
                : CameraFit.coordinates(
                    coordinates: points,
                    padding: const EdgeInsets.all(48),
                    maxZoom: osmMaxZoom.toDouble(),
                  ),
            maxZoom: osmMaxZoom.toDouble(),
          ),
          children: [
            TileLayer(
              urlTemplate: osmTiles,
              userAgentPackageName: 'org.bark.bark_app',
              maxNativeZoom: osmMaxZoom,
              tileProvider: _tiles,
              errorTileCallback: (_, error, _) {
                if (_noTiles || !mounted) return;
                // Once a screen: why, for whoever reads the log.
                _s.note('Map: no tile: $error');
                setState(() => _noTiles = true);
              },
            ),
            if (own != null)
              PolylineLayer(
                polylines: [
                  for (final m in t.list.where((m) => _placed(m.position)))
                    Polyline(
                      points: [own, _at(m.position)],
                      color: qualityColor(m.quality(t.floorDbm)),
                      strokeWidth: 4,
                      // A link the node avoids for now, dashed.
                      pattern: m.demoted || m.banned
                          ? StrokePattern.dashed(segments: const [10, 8])
                          : const StrokePattern.solid(),
                    ),
                ],
              ),
            if (own == null &&
                you != null &&
                anchor != null &&
                _placed(anchor.position))
              PolylineLayer(
                polylines: [
                  Polyline(
                    points: [you, _at(anchor.position)],
                    color: Theme.of(context).colorScheme.outline,
                    strokeWidth: 1.5,
                  ),
                ],
              ),
            if (you != null)
              MarkerLayer(
                markers: [
                  Marker(
                    point: you,
                    width: 18,
                    height: 18,
                    child: Semantics(label: tr.you, child: _youDot(context)),
                  ),
                ],
              ),
            // Grouped anew as the camera moves, so that zooming in
            // separates nodes that are near but not at one place.
            Builder(
              builder: (context) {
                final spots = _spots(context, t, drawn);
                final camera = MapCamera.of(context);
                final groups = places([
                  for (final s in spots) camera.latLngToScreenOffset(s.point),
                ], kMinInteractiveDimension);
                return MarkerLayer(
                  // The node most likely wanted drawn last, on top.
                  markers: [
                    for (final g in groups.reversed)
                      _marker(context, [for (final i in g) spots[i]]),
                  ],
                );
              },
            ),
            SimpleAttributionWidget(source: Text(tr.mapAttribution)),
          ],
        ),
        Positioned(
          left: 8,
          right: 8,
          top: 8,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (own == null)
                _banner(
                  you == null ? tr.mapNoPlaceNoLines : tr.mapNoPlaceAtYou,
                ),
              if (unplaced > 0 && drawn != null)
                _banner(tr.mapNeighboursNoPlace(unplaced)),
              if (_noTiles) _banner(tr.mapNoTiles),
              if (_error case final e?) _banner(tr.mapLastReadFailed(said(e))),
              _legend(t),
            ],
          ),
        ),
      ],
    );
  }

  /// The nodes on the map in the order a person most likely wants one
  /// where several share a place: this node, then relays, then sleepy
  /// nodes, each kind in the table's order.
  List<_Spot> _spots(BuildContext context, NeighbourTable t, LatLng? own) {
    final app = widget.app;
    _Spot neighbour(Neighbour m) {
      final said = _s.answers[hex(m.id)];
      final look = _NeighbourMarker(m, t, own, said);
      return _Spot(
        _placed(m.position) ? _at(m.position) : own!,
        width: said == null
            ? _NeighbourMarker.size
            : _NeighbourMarker.labelledWidth,
        height: said == null
            ? _NeighbourMarker.size
            : _NeighbourMarker.labelledHeight,
        name: nodeLabel(m.id),
        people: peopleLine(app, m.id),
        kind: m.carried
            ? tr.factCarried
            : m.sleepy
            ? tr.mapKindSleepy
            : tr.mapKindRelay,
        icon: look.icon,
        color: qualityColor(m.quality(t.floorDbm)),
        look: look.build,
        sheet: () => Watch(widget.app, (_) => _neighbourSheet(m, t)),
        onOpen: () => _askOne(m),
      );
    }

    final id = _s.node!.id;
    return [
      if (own != null)
        _Spot(
          own,
          width: _NodeMarker.size,
          height: _NodeMarker.size,
          name: nodeLabel(id),
          people: peopleLine(app, id),
          kind: tr.mapThisNode,
          icon: _NodeMarker.icon,
          color: Theme.of(context).colorScheme.primary,
          look: (count) => _NodeMarker.look(context, count),
          sheet: () => _Sheet(
            peopleLines(app, id),
            nodeLabel(id),
            _NodeMarker(widget.app, t).facts(),
          ),
        ),
      // One with no place is drawn with this node, if that is drawn.
      for (final m in t.list)
        if (!m.sleepy && (own != null || _placed(m.position))) neighbour(m),
      for (final m in t.list)
        if (m.sleepy && (own != null || _placed(m.position))) neighbour(m),
    ];
  }

  /// Asks `m` alone, as its sheet opens or the person asks again; by the
  /// rule of [Asks], not when its asks are under way or it answered lately.
  void _askOne(Neighbour m) {
    if (!_readable) return;
    _s.askNeighbours([m]).catchError((Object e) => _s.note('Ask: $e'));
  }

  /// A neighbour's sheet as it stands now, from the table read last, or
  /// `then`'s if the link is gone: it changes as the answer comes or the
  /// asks run out.
  Widget _neighbourSheet(Neighbour was, NeighbourTable then) {
    final table = _s.neighbours ?? then;
    final n =
        table.list.where((n) => hex(n.id) == hex(was.id)).firstOrNull ?? was;
    final placed = _s.status?.placed ?? false;
    final said = _s.answers[hex(n.id)];
    final look = _NeighbourMarker(
      n,
      table,
      placed ? _at(table.own) : null,
      said,
    );
    final facts = look.facts();
    final sleepy = said?.sleepy ?? n.sleepy;
    final line = said != null
        ? null
        : switch (_s.askState(n.id)) {
            AskState.asking => _Line(tr.mapAsking),
            AskState.gaveUp => _Line(
              sleepy ? tr.mapNoAnswerSleepy : tr.mapNoAnswer,
              again: _readable ? () => _askOne(n) : null,
            ),
            _ => null,
          };
    return _Sheet(
      peopleLines(widget.app, n.id),
      nodeLabel(n.id),
      facts,
      line: line,
      // Where its power would be, the first row an answer gives.
      lineAt: facts.indexWhere((f) => f.kind == FactKind.place) + 1,
    );
  }

  /// A place's marker: one node's, which a tap opens, or the first's with
  /// their count, which a tap lists.
  Marker _marker(BuildContext context, List<_Spot> here) {
    final first = here.first;
    final Widget child;
    if (here.length == 1) {
      child = GestureDetector(
        onTap: () => _open(context, first),
        child: first.look(1),
      );
    } else {
      child = Semantics(
        // Its own node, not merged into the map's.
        container: true,
        button: true,
        label: tr.mapNodesHereNamed(
          here.length,
          here.map((s) => s.name).join(', '),
        ),
        excludeSemantics: true,
        child: GestureDetector(
          onTap: () => _list(context, here),
          child: first.look(here.length),
        ),
      );
    }
    return Marker(
      point: first.point,
      width: first.width,
      height: first.height,
      child: child,
    );
  }

  void _open(BuildContext context, _Spot s) {
    s.onOpen?.call();
    showModalBottomSheet<void>(
      context: context,
      // Taller than half the screen when its rows need it.
      isScrollControlled: true,
      // "Last seen … ago" moves with the clock while the sheet is open.
      builder: (_) => Ticking((_) => s.sheet()),
    );
  }

  /// The nodes at one place, a row each, which opens its sheet.
  void _list(BuildContext context, List<_Spot> here) =>
      showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (sheet) => SafeArea(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(vertical: 16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Text(
                    tr.mapNodesHere(here.length),
                    style: Theme.of(sheet).textTheme.titleLarge,
                  ),
                ),
                const SizedBox(height: 8),
                for (final s in here)
                  Semantics(
                    button: true,
                    child: ListTile(
                      leading: Icon(s.icon, color: s.color),
                      title: Text('${s.name} · ${s.kind}'),
                      subtitle: s.people == null ? null : Text(s.people!),
                      onTap: () {
                        Navigator.pop(sheet);
                        _open(context, s);
                      },
                    ),
                  ),
              ],
            ),
          ),
        ),
      );

  Widget _banner(String s) => Card(
    child: Padding(padding: const EdgeInsets.all(8), child: Text(s)),
  );

  Widget _legend(NeighbourTable t) {
    Widget band(Quality q, String words) => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(width: 16, height: 4, color: qualityColor(q)),
        const SizedBox(width: 4),
        Text(words, style: const TextStyle(fontSize: 12)),
      ],
    );
    Widget kind(IconData icon, String words) => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(icon, size: 16),
        const SizedBox(width: 4),
        Text(words, style: const TextStyle(fontSize: 12)),
      ],
    );
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(8),
        child: Wrap(
          spacing: 12,
          runSpacing: 4,
          children: [
            Text(
              tr.mapNeighboursRead(t.list.length, when(t.at)),
              style: const TextStyle(fontSize: 12),
            ),
            band(Quality.good, tr.mapBandGood(_tenths(goodAboveFloorDb))),
            band(
              Quality.fair,
              tr.mapBandFair(
                _tenths(fairAboveFloorDb),
                _tenths(goodAboveFloorDb),
              ),
            ),
            band(Quality.fragile, tr.mapBandFragile(_tenths(fairAboveFloorDb))),
            kind(Icons.cell_tower, tr.mapLegendRelay),
            kind(Icons.battery_std, tr.mapLegendSleepy),
            kind(Icons.directions_walk, tr.mapLegendCarried),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Badge.count(count: 2),
                const SizedBox(width: 4),
                Text(tr.mapLegendShared, style: const TextStyle(fontSize: 12)),
              ],
            ),
            if (_you != null)
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  _youDot(context, size: 12),
                  const SizedBox(width: 4),
                  Flexible(
                    child: Text(
                      tr.mapLegendYou,
                      style: const TextStyle(fontSize: 12),
                    ),
                  ),
                ],
              ),
          ],
        ),
      ),
    );
  }

  /// The phone's own position, as the person asked to see it.
  static Widget _youDot(BuildContext context, {double size = 18}) => Container(
    width: size,
    height: size,
    decoration: BoxDecoration(
      color: Colors.blue,
      shape: BoxShape.circle,
      border: Border.all(color: Colors.white, width: 2),
    ),
  );

  /// Where the phone is, once the person asked; kept in this screen only.
  LatLng? _you;

  /// Asks the phone where it is, which asks for the location permission
  /// the first time; without one the map stays as it was.
  Future<void> _showMe() async {
    try {
      final p = await widget.app.place.here();
      if (mounted) setState(() => _you = _at(p));
    } on NoPlace catch (e) {
      if (mounted) say(context, said(e));
    }
  }
}

/// The paired node: its facts and its marker.
class _NodeMarker {
  final AppModel app;
  final NeighbourTable t;
  const _NodeMarker(this.app, this.t);

  /// Its facts from its status, in the order a neighbour's take.
  List<Fact> facts() {
    final s = app.session;
    final st = s.status ?? const Status();
    final anchor = t.list.where((n) => n.anchor).firstOrNull;
    return [
      _role(st.sleepy),
      _moving(st.carried),
      if (st.sleepy)
        _anchored(
          !st.anchorless,
          to: anchor == null ? null : nodeLabel(anchor.id),
        ),
      // A sleepy node with an anchor has no place of its own: others reach
      // it at its anchor's, its address.
      if (!_placed(t.own) && st.sleepy && st.placed)
        Fact(
          Icons.anchor,
          FactKind.place,
          tr.factPlaceAnchorsName,
          tr.factPlaceAnchors(_where(st.address)),
        )
      else
        _position(t.own, mapPoint: st.mapPoint),
      _power(
        usb: st.externalPower,
        reading: st.batteryMv == 0 ? null : tr.millivolts(st.batteryMv),
      ),
      Fact(Icons.hub, FactKind.known, tr.factKnownName, knownOf(t)),
      gpsFact(st.gpsCn0),
      // The node's screen's own form of a version.
      Fact(
        Icons.sell,
        FactKind.version,
        tr.factVersionName,
        'V${s.node!.image}',
      ),
      Fact(
        Icons.smartphone,
        FactKind.paired,
        tr.factPairedName,
        tr.factPairedWithThis,
      ),
    ];
  }

  static const size = 44.0;
  static const icon = Icons.my_location;

  /// Its marker, with `count` when it stands for that many at its place.
  static Widget look(BuildContext context, int count) => _counted(
    Icon(
      icon,
      size: 36,
      color: Theme.of(context).colorScheme.primary,
      semanticLabel: tr.mapThisNode,
    ),
    count,
  );
}

/// A neighbour: its facts and its marker.
class _NeighbourMarker {
  final Neighbour n;
  final NeighbourTable t;
  final LatLng? own;

  /// What it answered when asked, if it did.
  final NeighbourStatus? said;
  const _NeighbourMarker(this.n, this.t, this.own, this.said);

  static const size = 40.0;
  static const _label = 14.0;

  /// With its answer's first line under the icon; as much again above,
  /// so that the icon stays centred on its place.
  static const labelledWidth = 120.0;
  static const labelledHeight = size + 2 * _label;

  /// Its facts, in an order that does not change: what it is, from its
  /// answer or else its heartbeats, then what this node hears of it and
  /// what its answer says, then this node's view of it, which its screen
  /// cannot say.
  List<Fact> facts() {
    final q = n.quality(t.floorDbm);
    final far = own == null || !_placed(n.position)
        ? null
        : const Distance().as(LengthUnit.Meter, own!, _at(n.position));
    final s = said;
    final sleepy = s?.sleepy ?? n.sleepy;
    return [
      _role(sleepy),
      _moving(s?.carried ?? n.carried),
      if (sleepy)
        _anchored(s?.anchored ?? !n.anchorless)
      else if (n.anchor)
        Fact(
          Icons.anchor,
          FactKind.anchor,
          tr.factAnchorName,
          tr.factThisNodesAnchor,
        ),
      _position(n.position, mapPoint: n.mapPoint),
      if (s != null)
        _power(
          usb: s.usb,
          low: s.battery == NeighbourStatus.batteryLow,
          reading: s.percent == null ? null : tr.factPercent(s.percent!),
        ),
      // Good, fair or fragile by the margin over the noise floor, at the
      // thresholds of spec "Phone link", Neighbours ([goodAboveFloorDb],
      // [fairAboveFloorDb]); the numbers go to the row's help.
      Fact(
        Icons.settings_input_antenna,
        FactKind.link,
        _qualitySaid(q),
        qualityName(q),
        color: qualityColor(q),
        detail: tr.factLinkDetail(
          _signed(n.rssiDbm),
          _signed(n.rssiDbm - t.floorDbm, plus: true),
        ),
      ),
      Fact(
        Icons.schedule,
        FactKind.lastHeard,
        tr.factLastHeardName,
        secondsAgo(n.ageS),
      ),
      if (far != null)
        Fact(
          Icons.straighten,
          FactKind.distance,
          tr.factDistanceName,
          distance(far),
        ),
      if (s != null) ...[
        Fact(
          Icons.hub,
          FactKind.known,
          tr.factKnownName,
          known(s.heard, s.strong, s.proven),
        ),
        gpsFact(s.gpsCn0),
        // The versions in the node's screen's own form.
        if (s.version != 0)
          Fact(
            Icons.sell,
            FactKind.version,
            tr.factVersionName,
            s.gathers == 0
                ? 'V${s.version}'
                : tr.factVersionGetting(
                    'V${s.version}',
                    'V${s.gathers}',
                    s.complete,
                    s.blocks,
                  ),
          ),
      ],
      if (n.proven)
        Fact(
          Icons.check_box,
          FactKind.exchanged,
          tr.factExchangedName,
          tr.factExchangedValue,
        ),
      if (n.demoted)
        Fact(
          Icons.trending_down,
          FactKind.demoted,
          tr.factDemotedName,
          tr.factDemotedValue,
        ),
      if (n.banned)
        Fact(
          Icons.block,
          FactKind.avoided,
          tr.factAvoidedName,
          tr.factAvoidedValue,
        ),
    ];
  }

  IconData get icon => n.carried
      ? Icons.directions_walk
      : n.sleepy
      ? Icons.battery_std
      : Icons.cell_tower;

  /// Its marker, with `count` when it stands for that many at its place.
  Widget build(int count) {
    final icon = _counted(
      Icon(
        this.icon,
        size: 32,
        color: qualityColor(n.quality(t.floorDbm)),
        semanticLabel: nodeLabel(n.id),
      ),
      count,
    );
    final s = said;
    return s == null
        ? icon
        : Builder(
            builder: (context) => Column(
              children: [
                const SizedBox(height: _label),
                SizedBox(
                  height: size,
                  child: Center(child: icon),
                ),
                Container(
                  height: _label,
                  padding: const EdgeInsets.symmetric(horizontal: 2),
                  color: Theme.of(context).colorScheme.surface,
                  child: Text(
                    s.role,
                    style: const TextStyle(fontSize: 10),
                    maxLines: 1,
                  ),
                ),
              ],
            ),
          );
  }
}

/// What a sheet says of a neighbour's asks while it has no answer, and
/// how to ask it again once they ran out.
class _Line extends StatelessWidget {
  final String text;
  final VoidCallback? again;
  const _Line(this.text, {this.again});

  @override
  Widget build(BuildContext context) => ConstrainedBox(
    constraints: const BoxConstraints(minHeight: kMinInteractiveDimension),
    child: Row(
      children: [
        Expanded(
          child: Text(
            text,
            style: TextStyle(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ),
        if (again != null)
          TextButton(onPressed: again, child: Text(tr.askAgain)),
      ],
    ),
  );
}

/// A node's details: its ID, the people at it ([peopleLines]), and a row
/// a fact, in plain words, none of its screen's, with `line` before the
/// row at `lineAt`.
class _Sheet extends StatelessWidget {
  final List<String> people;
  final String id;
  final List<Fact> facts;
  final Widget? line;
  final int lineAt;
  const _Sheet(this.people, this.id, this.facts, {this.line, this.lineAt = 0});

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return SafeArea(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(id, style: text.titleLarge),
            for (final p in people) Text(p),
            const SizedBox(height: 8),
            for (final (i, f) in facts.indexed) ...[
              if (i == lineAt) ?line,
              _row(context, f),
            ],
            if (lineAt >= facts.length) ?line,
          ],
        ),
      ),
    );
  }

  /// A row, tapped anywhere, opens what its fact means.
  Widget _row(BuildContext context, Fact f) {
    final row = InkWell(
      onTap: () => showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (context) => SafeArea(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(f.label, style: Theme.of(context).textTheme.titleLarge),
                const SizedBox(height: 8),
                Text(factHelp(f.kind)),
                if (f.detail != null) ...[
                  const SizedBox(height: 8),
                  Text(f.detail!),
                ],
              ],
            ),
          ),
        ),
      ),
      // The least tap target Material asks for.
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: kMinInteractiveDimension),
        child: Row(
          children: [
            Icon(f.icon, color: f.color, semanticLabel: f.name),
            const SizedBox(width: 12),
            // The icon names the fact to a screen reader already.
            ExcludeSemantics(child: Text(f.label)),
            const SizedBox(width: 16),
            Expanded(
              child: Text(f.value, textAlign: TextAlign.end, maxLines: 3),
            ),
          ],
        ),
      ),
    );
    return f.tooltip == null ? row : Tooltip(message: f.tooltip, child: row);
  }
}
