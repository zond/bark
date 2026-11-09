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
import '../link.dart';
import '../session.dart';
import 'common.dart';

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

/// What a node is, in the words of its screen's first line, from what
/// its heartbeats say when it has not answered: sleepy, its anchor
/// heard or not; or a relay with no place, carried or fixed.
String roleOf({
  required bool sleepy,
  required bool anchored,
  required bool placed,
  required bool carried,
}) => sleepy
    ? (anchored ? 'SLEEPY ANCHORED' : 'SLEEPY NO ANCHOR')
    : !placed
    ? 'RELAY NO PLACE'
    : carried
    ? 'CARRIED RELAY'
    : 'FIXED RELAY';

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
  const Fact(
    this.icon,
    this.label,
    this.name,
    this.value, {
    this.color,
    this.tooltip,
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

/// Whether a node is carried, said whatever its header's words leave
/// out, as a sleepy node's do (spec "Nodes").
Fact _moving(bool carried) => Fact(
  Icons.directions_walk,
  'Moving',
  'moving',
  carried ? 'carried' : 'stays put',
);

/// Where a node's position comes from: GPS, or a map point a person set;
/// none without a place.
Fact _position(Position p, {required bool mapPoint}) => !_placed(p)
    ? const Fact(Icons.location_off, 'Place', 'position', 'no place')
    : mapPoint
    ? Fact(Icons.push_pin, 'Place', 'position from a map point', _where(p))
    : Fact(Icons.satellite_alt, 'Place', 'position from GPS', _where(p));

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

  bool get _readable =>
      _s.connected && !_s.guest && (_s.node?.version ?? 0) >= neighboursVersion;

  Future<void> _read() async {
    if (!_readable) return;
    try {
      final t = await _s.readNeighbours();
      if (mounted) setState(() => _error = null);
      final link = _s.link;
      if ((_ask || link != _askedOn) &&
          mounted &&
          (_s.node?.version ?? 0) >= askVersion) {
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
    if (n.version < neighboursVersion) {
      return _say(
        'This node\'s firmware cannot tell its neighbours (link version '
        '${n.version}): update it to see the map.',
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
                    child: _NodeMarker(_s, t),
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
      child: _NeighbourMarker(m, t, own, said),
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
            kind(Icons.cell_tower, 'a node on power'),
            kind(Icons.battery_std, 'a node on battery'),
            kind(Icons.directions_walk, 'a carried node'),
          ],
        ),
      ),
    );
  }
}

class _NodeMarker extends StatelessWidget {
  final Session s;
  final NeighbourTable t;
  const _NodeMarker(this.s, this.t);

  @override
  Widget build(BuildContext context) => GestureDetector(
    onTap: () {
      final st = s.status ?? const Status();
      showModalBottomSheet<void>(
        context: context,
        // Taller than half the screen when its rows need it.
        isScrollControlled: true,
        builder: (_) => _Sheet(
          nodeLabel(s.node!.id),
          roleOf(
            sleepy: st.sleepy,
            anchored: !st.anchorless,
            placed: st.placed,
            carried: st.carried,
          ),
          [
            const Fact(Icons.smartphone, 'Paired', 'paired', 'with this phone'),
            _power(
              usb: st.externalPower,
              battery: st.batteryMv == 0 ? null : '${st.batteryMv} mV',
            ),
            _moving(st.carried),
            Fact(Icons.hub, 'Known', 'neighbours known', knownOf(t)),
            Fact(
              Icons.sell,
              'Version',
              'firmware version',
              'V${s.node!.image}',
            ),
            _position(t.own, mapPoint: st.mapPoint),
          ],
        ),
      );
    },
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
  const _NeighbourMarker(this.n, this.t, this.own, this.said);

  static const size = 40.0;
  static const _label = 14.0;

  /// With its answer's first line under the icon; as much again above,
  /// so that the icon stays centred on its place.
  static const labelledWidth = 120.0;
  static const labelledHeight = size + 2 * _label;

  /// Its facts, in an order that does not change: what this node hears
  /// of it, then what its answer says, then this node's view of it,
  /// which its screen cannot say.
  List<Fact> facts() {
    final q = n.quality(t.floorDbm);
    final far = own == null
        ? null
        : const Distance().as(LengthUnit.Meter, own!, _at(n.position));
    final s = said;
    return [
      Fact(
        Icons.settings_input_antenna,
        'Link',
        'link strength, ${qualityName(q)}',
        '${_signed(n.rssiDbm)} dBm \u00b7 '
            '${_signed(n.rssiDbm - t.floorDbm, plus: true)} dB',
        color: qualityColor(q),
        tooltip: 'signal, and its margin over the noise floor',
      ),
      Fact(Icons.schedule, 'Last heard', 'last heard', secondsAgo(n.ageS)),
      if (far != null)
        Fact(Icons.straighten, 'Distance', 'distance', distance(far)),
      if (s != null)
        _power(
          usb: s.usb,
          battery: s.battery == NeighbourStatus.batteryLow
              ? 'low'
              : s.percent == null
              ? null
              : '${s.percent}%',
        ),
      _moving(s?.carried ?? n.carried),
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
      _position(n.position, mapPoint: n.mapPoint),
      if (n.proven)
        const Fact(
          Icons.check_box,
          'Exchanged',
          'exchanged',
          'it took a message from this node',
        ),
      if (n.anchor)
        const Fact(Icons.anchor, 'Anchor', 'anchor', 'this node\'s'),
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
        builder: (_) => _Sheet(
          nodeLabel(n.id),
          s?.role ??
              roleOf(
                sleepy: n.sleepy,
                anchored: !n.anchorless,
                placed: _placed(n.position),
                carried: n.carried,
              ),
          facts(),
        ),
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

/// A node's details: its ID, what it is in its screen's words, and a row
/// a fact.
class _Sheet extends StatelessWidget {
  final String title;
  final String role;
  final List<Fact> facts;
  const _Sheet(this.title, this.role, this.facts);

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
            Text(title, style: text.titleLarge),
            Text(role, style: text.titleSmall),
            const SizedBox(height: 8),
            for (final f in facts) _row(f),
          ],
        ),
      ),
    );
  }

  Widget _row(Fact f) {
    final row = Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Icon(f.icon, color: f.color, semanticLabel: f.name),
          const SizedBox(width: 12),
          // The icon names the fact to a screen reader already.
          ExcludeSemantics(child: Text(f.label)),
          const SizedBox(width: 16),
          Expanded(child: Text(f.value, textAlign: TextAlign.end, maxLines: 2)),
        ],
      ),
    );
    return f.tooltip == null ? row : Tooltip(message: f.tooltip, child: row);
  }
}
