// The paired node and the neighbours it knows on a street map, a line to
// each coloured by the link's quality (spec "Phone link", Neighbours).
// Read when the screen opens, at every status the node gives and when
// the person asks. When it opens and when the person asks, each
// neighbour without a fresh answer is also asked what its own screen
// shows, and asked again while it does not answer (spec "Phone link",
// Ask), and its marker says it in those words.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_map/flutter_map.dart';
import 'package:latlong2/latlong.dart';

import '../app.dart';
import '../contacts.dart';
import '../link.dart';
import '../session.dart';
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

String qualityName(Quality q) => switch (q) {
  Quality.good => 'good',
  Quality.fair => 'fair',
  Quality.fragile => 'fragile',
};

bool _placed(Position p) => p.lat != 0 || p.lon != 0;

/// A signed number with the minus sign typography uses.
String _signed(num v, {bool plus = false}) {
  final r = v.round();
  return r < 0 ? '\u2212${-r}' : (plus ? '+$r' : '$r');
}

/// A distance in metres, in kilometres from 1 km.
String distance(double m) =>
    m < 1000 ? '${m.round()} m' : '${(m / 1000).toStringAsFixed(1)} km';

/// Seconds since, as a person says it.
String secondsAgo(int s) => s < 60 ? '$s s ago' : ago(Duration(seconds: s));

/// A position to five decimals, about a metre.
String _where(Position p) =>
    '${p.latDeg.toStringAsFixed(5)}, ${p.lonDeg.toStringAsFixed(5)}';

/// The neighbours a node knows as its screen's KNOWN line counts them:
/// heard, heard with the margin a full frame needs through a fade, and
/// those that have taken a message from it (spec "Neighbour status").
String known(int heard, int strong, int exchanged) =>
    '$heard heard \u00b7 $strong strong \u00b7 $exchanged exchanged';

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
  final String label;
  final String name;
  final String value;
  final Color? color;

  /// Said on a long press, for a value whose parts need naming.
  final String? tooltip;

  /// This node's own figures, shown under the row's help.
  final String? detail;
  const Fact(
    this.icon,
    this.label,
    this.name,
    this.value, {
    this.color,
    this.tooltip,
    this.detail,
  });
}

/// The facts of a node's power, as its answer or status says it.
Fact _power({required bool usb, String? battery}) => usb
    ? const Fact(Icons.power, 'Power', 'power', 'USB')
    : Fact(
        switch (battery) {
          null => Icons.battery_unknown,
          'low' => Icons.battery_alert,
          _ => Icons.battery_std,
        },
        'Power',
        'battery',
        battery ?? 'not read',
      );

/// What a node does for others: a relay passes texts on; a sleepy node
/// sleeps between windows and sends and receives through its anchor
/// (spec "Nodes").
Fact _role(bool sleepy) => sleepy
    ? const Fact(
        Icons.bedtime,
        'Role',
        'role, sleepy',
        'sleepy: wakes in windows, speaks through its anchor',
      )
    : const Fact(
        Icons.cell_tower,
        'Role',
        'role, relay',
        'relay: passes on others\' texts',
      );

/// Whether a node is carried (spec "Nodes").
Fact _moving(bool carried) => carried
    ? const Fact(Icons.directions_walk, 'Moving', 'moving', 'carried')
    : const Fact(Icons.house, 'Moving', 'moving', 'stays put');

/// A sleepy node's anchor: `to` names it when known, as for this node's
/// own, whose anchor is the neighbour the table marks; a neighbour's
/// anchor the link does not name (spec "Phone link", Neighbours).
Fact _anchored(bool anchored, {String? to}) => anchored
    ? Fact(
        Icons.anchor,
        'Anchor',
        'anchor',
        to == null ? 'anchored' : 'anchored to $to',
      )
    : const Fact(Icons.link_off, 'Anchor', 'anchor', 'no anchor yet');

/// Where a node's position comes from, GPS or a map point a person set,
/// and the position on a second line; none without a place.
Fact _position(Position p, {required bool mapPoint}) => !_placed(p)
    ? const Fact(Icons.location_off, 'Place', 'place', 'no place yet')
    : mapPoint
    ? Fact(
        Icons.push_pin,
        'Place',
        'place, set by a person',
        'set by a person\n${_where(p)}',
      )
    : Fact(
        Icons.satellite_alt,
        'Place',
        'place, from GPS',
        'from GPS\n${_where(p)}',
      );

/// The names of the contacts whose home, or node last used, has `id`:
/// a node on the map is often a person's.
String? contactsAt(Contacts contacts, List<int> id) {
  bool at(Address? a) => a != null && hex(a.id) == hex(id);
  final names = [
    for (final c in contacts.active)
      if (at(c.card.home) || at(c.place)) c.name,
  ];
  return names.isEmpty ? null : names.join(', ');
}

LatLng _at(Position p) => LatLng(p.latDeg, p.lonDeg);

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
  String? _error;

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
      if (mounted) setState(() => _error = '$e');
    }
  }

  Future<void> _refresh() {
    _ask = true;
    return _read();
  }

  @override
  Widget build(BuildContext context) => Watch(widget.app, (context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Nearby nodes'),
        actions: [
          IconButton(
            tooltip: 'Read again',
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
      return _say('Not connected to a node: connect to see its neighbours.');
    }
    if (_s.guest) {
      return _say(
        'Connected as a guest: only the phone paired with this node sees '
        'its neighbours.',
      );
    }
    final t = _s.neighbours;
    if (t == null) {
      return _error != null
          ? _say('Reading the neighbours failed: $_error')
          : const Center(child: CircularProgressIndicator());
    }
    final placed = _s.status?.placed ?? false;
    final own = placed ? _at(t.own) : null;
    final points = [?own, for (final m in t.list) _at(m.position)];
    if (points.isEmpty) {
      return _say(
        'The node has no place yet and has heard no neighbour: nothing '
        'to draw.',
      );
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
              errorTileCallback: (_, _, _) {
                if (!_noTiles && mounted) setState(() => _noTiles = true);
              },
            ),
            if (own != null)
              PolylineLayer(
                polylines: [
                  for (final m in t.list)
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
            MarkerLayer(
              markers: [
                for (final m in t.list) _neighbour(m, t, own),
                if (own != null)
                  Marker(
                    point: own,
                    width: 44,
                    height: 44,
                    child: _NodeMarker(widget.app, t),
                  ),
              ],
            ),
            const SimpleAttributionWidget(
              source: Text('© OpenStreetMap contributors'),
            ),
          ],
        ),
        Positioned(
          left: 8,
          right: 8,
          top: 8,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (!placed)
                _banner(
                  'The node has no place yet: its neighbours are drawn '
                  'without lines to it.',
                ),
              if (_noTiles)
                _banner(
                  'No street map here: the phone has no internet and has '
                  'not kept this area from before. The nodes and links are '
                  'drawn on a blank background.',
                ),
              if (_error != null) _banner('The last read failed: $_error'),
              _legend(t),
            ],
          ),
        ),
      ],
    );
  }

  Marker _neighbour(Neighbour m, NeighbourTable t, LatLng? own) {
    final said = _s.answers[hex(m.id)];
    return Marker(
      point: _at(m.position),
      width: said == null
          ? _NeighbourMarker.size
          : _NeighbourMarker.labelledWidth,
      height: said == null
          ? _NeighbourMarker.size
          : _NeighbourMarker.labelledHeight,
      child: _NeighbourMarker(m, t, own, said, widget.app.contacts),
    );
  }

  Widget _banner(String s) => Card(
    child: Padding(padding: const EdgeInsets.all(8), child: Text(s)),
  );

  Widget _legend(NeighbourTable t) {
    final fair = fairAboveFloorDb.toStringAsFixed(1);
    final good = goodAboveFloorDb.toStringAsFixed(1);
    Widget band(Quality q, String range) => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(width: 16, height: 4, color: qualityColor(q)),
        const SizedBox(width: 4),
        Text('${qualityName(q)} $range', style: const TextStyle(fontSize: 12)),
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
              '${t.list.length} neighbours, read ${when(t.at)}',
              style: const TextStyle(fontSize: 12),
            ),
            band(Quality.good, '≥ $good dB'),
            band(Quality.fair, '$fair–$good dB'),
            band(Quality.fragile, '< $fair dB over the noise floor'),
            kind(Icons.cell_tower, 'a relay'),
            kind(Icons.battery_std, 'a sleepy node'),
            kind(Icons.directions_walk, 'a carried node'),
          ],
        ),
      ),
    );
  }
}

class _NodeMarker extends StatelessWidget {
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
      _position(t.own, mapPoint: st.mapPoint),
      _power(
        usb: st.externalPower,
        battery: st.batteryMv == 0 ? null : '${st.batteryMv} mV',
      ),
      Fact(Icons.hub, 'Known', 'neighbours known', knownOf(t)),
      Fact(Icons.sell, 'Version', 'firmware version', 'V${s.node!.image}'),
      const Fact(Icons.smartphone, 'Paired', 'paired', 'with this phone'),
    ];
  }

  @override
  Widget build(BuildContext context) => GestureDetector(
    onTap: () => showModalBottomSheet<void>(
      context: context,
      // Taller than half the screen when its rows need it.
      isScrollControlled: true,
      builder: (_) => _Sheet(
        contactsAt(app.contacts, app.session.node!.id),
        nodeLabel(app.session.node!.id),
        facts(),
      ),
    ),
    child: Icon(
      Icons.my_location,
      size: 36,
      color: Theme.of(context).colorScheme.primary,
      semanticLabel: 'this node',
    ),
  );
}

class _NeighbourMarker extends StatelessWidget {
  final Neighbour n;
  final NeighbourTable t;
  final LatLng? own;

  /// What it answered when asked, if it did.
  final NeighbourStatus? said;
  final Contacts contacts;
  const _NeighbourMarker(this.n, this.t, this.own, this.said, this.contacts);

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
    final far = own == null
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
        const Fact(Icons.anchor, 'Anchor', 'anchor', 'this node\'s anchor'),
      _position(n.position, mapPoint: n.mapPoint),
      if (s != null)
        _power(
          usb: s.usb,
          battery: s.battery == NeighbourStatus.batteryLow
              ? 'low'
              : s.percent == null
              ? null
              : '${s.percent}%',
        ),
      // Good, fair or fragile by the margin over the noise floor, at the
      // thresholds of spec "Phone link", Neighbours ([goodAboveFloorDb],
      // [fairAboveFloorDb]); the numbers go to the row's help.
      Fact(
        Icons.settings_input_antenna,
        'Link',
        'link strength, ${qualityName(q)}',
        qualityName(q),
        color: qualityColor(q),
        detail:
            'This link: ${_signed(n.rssiDbm)} dBm, '
            '${_signed(n.rssiDbm - t.floorDbm, plus: true)} dB over the '
            'noise floor.',
      ),
      Fact(Icons.schedule, 'Last heard', 'last heard', secondsAgo(n.ageS)),
      if (far != null)
        Fact(Icons.straighten, 'Distance', 'distance', distance(far)),
      if (s != null) ...[
        Fact(
          Icons.hub,
          'Known',
          'neighbours known',
          known(s.heard, s.strong, s.proven),
        ),
        if (s.version != 0)
          Fact(
            Icons.sell,
            'Version',
            'firmware version',
            s.gathers == 0
                ? 'V${s.version}'
                : 'V${s.version} \u00b7 getting V${s.gathers} '
                      '${s.complete}/${s.blocks}',
          ),
      ],
      if (n.proven)
        const Fact(
          Icons.check_box,
          'Exchanged',
          'exchanged',
          'it took a message from this node',
        ),
      if (n.demoted)
        const Fact(
          Icons.trending_down,
          'Demoted',
          'demoted',
          'its last hand-overs failed',
        ),
      if (n.banned)
        const Fact(
          Icons.block,
          'Banned',
          'banned',
          'for now: it refused or failed a hand-over',
        ),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final q = n.quality(t.floorDbm);
    final icon = Icon(
      n.carried
          ? Icons.directions_walk
          : n.sleepy
          ? Icons.battery_std
          : Icons.cell_tower,
      size: 32,
      color: qualityColor(q),
      semanticLabel: nodeLabel(n.id),
    );
    final s = said;
    return GestureDetector(
      onTap: () => showModalBottomSheet<void>(
        context: context,
        // Taller than half the screen when its rows need it.
        isScrollControlled: true,
        builder: (_) =>
            _Sheet(contactsAt(contacts, n.id), nodeLabel(n.id), facts()),
      ),
      child: s == null
          ? icon
          : Column(
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

/// A node's details: whose it is, if a contact's, its ID, and a row a
/// fact, in plain words, none of its screen's.
class _Sheet extends StatelessWidget {
  final String? whose;
  final String id;
  final List<Fact> facts;
  const _Sheet(this.whose, this.id, this.facts);

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
            if (whose != null) Text(whose!, style: text.titleLarge),
            Text(id, style: whose == null ? text.titleLarge : text.titleSmall),
            const SizedBox(height: 8),
            for (final f in facts) _row(context, f),
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
                Text(factHelp[f.label]!),
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
