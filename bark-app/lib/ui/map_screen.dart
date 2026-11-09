// The paired node and the neighbours it knows on a street map, a line to
// each coloured by the link's quality (spec "Phone link", Neighbours).
// Read when the screen opens, at every status the node gives and when
// the person asks. When it opens and when the person asks, each
// neighbour is also asked what its own screen shows (spec "Phone link",
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

/// The flags of a neighbour, in words.
List<String> neighbourFlags(Neighbour n) => [
  if (n.anchor) 'this node\'s anchor',
  if (n.sleepy) 'sleepy',
  if (n.carried) 'carried',
  if (n.mapPoint) 'position from a map point',
  if (n.anchorless) 'anchorless',
  if (n.demoted) 'demoted: its last hand-overs failed',
  if (n.banned) 'banned for now: it refused or failed a hand-over',
  if (n.proven) 'has taken a message from this node',
];

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
  /// that succeeds: once when the map opens, perhaps before the node is
  /// connected, and once each time the person refreshes it. Not at every
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
        title: const Text('Map'),
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
                    child: _NodeMarker(_s),
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
          ],
        ),
      ),
    );
  }
}

class _NodeMarker extends StatelessWidget {
  final Session s;
  const _NodeMarker(this.s);

  @override
  Widget build(BuildContext context) => GestureDetector(
    onTap: () => showModalBottomSheet<void>(
      context: context,
      builder: (_) => _Sheet(nodeName(s.node!.id), [
        'The node this phone is paired with',
        if (s.status?.sleepy ?? false) 'Sleepy',
        if (s.status?.carried ?? false) 'Carried',
        'Relays known: ${s.status?.relays ?? 0}',
      ]),
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
  const _NeighbourMarker(this.n, this.t, this.own, this.said);

  static const size = 40.0;
  static const _label = 14.0;

  /// With its answer's first line under the icon; as much again above,
  /// so that the icon stays centred on its place.
  static const labelledWidth = 120.0;
  static const labelledHeight = size + 2 * _label;

  @override
  Widget build(BuildContext context) {
    final q = n.quality(t.floorDbm);
    final margin = n.rssiDbm - t.floorDbm;
    final far = own == null
        ? null
        : const Distance().as(LengthUnit.Meter, own!, _at(n.position));
    final icon = Icon(
      n.carried
          ? Icons.directions_walk
          : n.sleepy
          ? Icons.battery_std
          : Icons.cell_tower,
      size: 32,
      color: qualityColor(q),
      semanticLabel: nodeName(n.id),
    );
    final s = said;
    return GestureDetector(
      onTap: () => showModalBottomSheet<void>(
        context: context,
        builder: (_) => _Sheet(nodeName(n.id), [
          'Link ${qualityName(q)}: ${n.rssiDbm.toStringAsFixed(1)} dBm, '
              '${margin.toStringAsFixed(1)} dB over the noise floor',
          'Last heard ${n.ageS} s ago',
          if (far != null) '${far.round()} m away',
          // Its own screen's lines, one a row, say what the flags would,
          // from the node itself.
          if (s != null) ...s.lines else ...neighbourFlags(n),
        ]),
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

class _Sheet extends StatelessWidget {
  final String title;
  final List<String> lines;
  const _Sheet(this.title, this.lines);

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title, style: Theme.of(context).textTheme.titleLarge),
          const SizedBox(height: 8),
          for (final l in lines) Text(l),
        ],
      ),
    ),
  );
}
