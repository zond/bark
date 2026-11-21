//! The transport on Linux: BlueZ, the Linux Bluetooth daemon, driven over
//! the system D-Bus (BlueZ's `doc/org.bluez.*.rst`). Every call and every
//! error BlueZ answers is logged with `-v`, verbatim, so that a link that
//! fails can be read from the log.

use std::collections::HashMap;
use std::io::BufRead;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::MatchRule;

use crate::link::{id_of_name, uuid_text, Char, Error, Found, Link, Result, Transport};

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const SERVICE: &str = "org.bluez.GattService1";
const CHARACTERISTIC: &str = "org.bluez.GattCharacteristic1";
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECTS: &str = "org.freedesktop.DBus.ObjectManager";

/// Where the tool's pairing agent lives on the bus.
const AGENT_PATH: &str = "/org/bark/agent";

/// The longest any call to BlueZ is waited for: a pairing, while the
/// person types the code. The Security Manager gives up 30 s after any
/// step (Vol 3 Part H §3.4), and a pairing has a few; four times that only
/// keeps the tool from waiting for ever. A read or a write BlueZ gives up
/// itself after the ATT transaction's 30 s (Vol 3 Part F §3.3.3).
const CALL_TIMEOUT: Duration = Duration::from_secs(4 * 30);

/// How long one attempt to connect is waited for: ten of the node's
/// advertising intervals of 1022.5 ms (spec "Phone link", Finding the
/// node). The connection request answers an advertisement, so a node in
/// reach is caught within a few intervals; past ten, BlueZ is told to stop
/// and the tool tries again.
const CONNECT_WAIT: Duration = Duration::from_micros(10 * 1_022_500);

/// How long the GATT discovery may take after the link is up. BlueZ
/// discovers the whole table before it says the services are resolved;
/// the node's table is three services, a few round trips.
const RESOLVE_WAIT: Duration = Duration::from_secs(30);

/// How often a scan asks whether it has heard enough while nothing new
/// comes: a tenth of an advertising interval.
const SCAN_TICK: Duration = Duration::from_millis(100);

type Props = HashMap<String, OwnedValue>;
type Interfaces = HashMap<String, Props>;
type Objects = HashMap<OwnedObjectPath, Interfaces>;

/// A signal BlueZ sent: an object added, or its properties changed.
enum Signal {
    Added(OwnedObjectPath, Interfaces),
    Changed(OwnedObjectPath, String, Props),
}

struct Bus {
    conn: Connection,
    /// For Connect alone, whose wait is its own ([`CONNECT_WAIT`]).
    connecting: Connection,
    /// 1 logs the steps and the nodes; 2 every device heard too.
    verbose: u8,
    signals: Receiver<Signal>,
}

impl Bus {
    fn say(&self, level: u8, s: impl FnOnce() -> String) {
        if self.verbose >= level {
            eprintln!("bluez: {}", s());
        }
    }

    fn call<B>(
        &self,
        path: &str,
        iface: &str,
        method: &str,
        body: &B,
        shown: &str,
    ) -> Result<zbus::Message>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        self.call_on(&self.conn, path, iface, method, body, shown)
    }

    fn call_on<B>(
        &self,
        conn: &Connection,
        path: &str,
        iface: &str,
        method: &str,
        body: &B,
        shown: &str,
    ) -> Result<zbus::Message>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        self.say(1, || format!("call {iface}.{method}({shown}) on {path}"));
        match conn.call_method(Some(BLUEZ), path, Some(iface), method, body) {
            Ok(m) => Ok(m),
            Err(e) => {
                self.say(1, || {
                    format!("error from {iface}.{method}: {}", verbatim(&e))
                });
                Err(error_of(&e))
            }
        }
    }

    fn objects(&self) -> Result<Objects> {
        let m = self.call("/", OBJECTS, "GetManagedObjects", &(), "")?;
        m.body()
            .deserialize()
            .map_err(|e| Error::Transport(e.to_string()))
    }

    fn properties(&self, path: &str, iface: &str) -> Result<Props> {
        let m = self.call(path, PROPERTIES, "GetAll", &(iface,), iface)?;
        m.body()
            .deserialize()
            .map_err(|e| Error::Transport(e.to_string()))
    }

    /// The next signal, until `deadline`.
    fn next(&self, deadline: Instant) -> Option<Signal> {
        let left = deadline.saturating_duration_since(Instant::now());
        self.signals.recv_timeout(left).ok()
    }

    fn remove(&self, adapter: &str, device: &str) -> Result<()> {
        let path = OwnedObjectPath::try_from(device.to_string())
            .map_err(|e| Error::Transport(e.to_string()))?;
        self.call(adapter, ADAPTER, "RemoveDevice", &(path,), device)
            .map(|_| ())
    }
}

/// D-Bus's error, as BlueZ wrote it.
fn verbatim(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(name, text, _) => {
            format!("{} {}", name.as_str(), text.as_deref().unwrap_or(""))
        }
        e => e.to_string(),
    }
}

/// The ATT error in BlueZ's answer to a read or a write, where it says
/// one (its `src/gatt-client.c`, `create_gatt_dbus_error`): most codes in
/// its text, and some by words of their own.
fn error_of(e: &zbus::Error) -> Error {
    match e {
        zbus::Error::MethodError(name, text, _) => {
            let text = text.as_deref().unwrap_or("");
            if let Some(code) = text
                .split_once("ATT error: 0x")
                .and_then(|(_, h)| u8::from_str_radix(h.get(..2)?, 16).ok())
            {
                return Error::Att(code);
            }
            // BlueZ says "Not paired" for insufficient authentication and
            // for the encryption errors alike.
            if text == "Not paired" {
                return Error::Att(0x05);
            }
            if text == "Invalid Length" {
                return Error::Att(bark_link::att::INVALID_LENGTH);
            }
            if name.as_str() == "org.bluez.Error.NotConnected" || text == "Not connected" {
                return Error::Ended;
            }
            Error::Transport(verbatim(e))
        }
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            Error::TimedOut(verbatim(e))
        }
        e => Error::Transport(verbatim(e)),
    }
}

fn string(p: &Props, key: &str) -> Option<String> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

fn strings(p: &Props, key: &str) -> Vec<String> {
    p.get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .unwrap_or_default()
}

fn boolean(p: &Props, key: &str) -> Option<bool> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

fn int16(p: &Props, key: &str) -> Option<i16> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

fn uint16(p: &Props, key: &str) -> Option<u16> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

fn path_of(p: &Props, key: &str) -> Option<OwnedObjectPath> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

fn bytes(p: &Props, key: &str) -> Option<Vec<u8>> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

/// A device as the scan has it so far.
#[derive(Default)]
struct Device {
    address: String,
    name: Option<String>,
    rssi: Option<i16>,
    uuids: Vec<String>,
    paired: bool,
    /// Heard since the scan began, not only known from before.
    heard: bool,
}

impl Device {
    fn update(&mut self, p: &Props) {
        if let Some(a) = string(p, "Address") {
            self.address = a;
        }
        if let Some(n) = string(p, "Name") {
            self.name = Some(n);
        }
        if let Some(r) = int16(p, "RSSI") {
            self.rssi = Some(self.rssi.map_or(r, |was| was.max(r)));
        }
        if let Some(b) = boolean(p, "Paired") {
            self.paired = b;
        }
        for u in strings(p, "UUIDs") {
            if !self.uuids.contains(&u) {
                self.uuids.push(u);
            }
        }
    }

    /// The node it is, if it is one: named as a node. Public when it
    /// advertised the public node's UUID, which no attribute has (spec
    /// "Phone link", Public): BlueZ adds to a device's UUIDs the services
    /// it finds on a link, so the service's own UUID there does not tell a
    /// private node.
    fn found(&self) -> Option<Found> {
        Some(Found {
            id: id_of_name(self.name.as_deref()?)?,
            address: self.address.clone(),
            rssi: self.rssi,
            public: self
                .uuids
                .contains(&uuid_text(bark_link::uuid::PUBLIC_NODE)),
            bonded: self.paired,
        })
    }
}

pub struct Bluez {
    bus: Rc<Bus>,
    adapter: String,
}

impl Bluez {
    /// On the system bus, with the adapter named `adapter`, such as
    /// `hci0`, or else the first BlueZ lists; `verbose` as [`Bus`] says.
    pub fn new(adapter: Option<&str>, verbose: u8) -> Result<Bluez> {
        let bus_with = |timeout| {
            zbus::blocking::connection::Builder::system()
                .and_then(|b| b.method_timeout(timeout).build())
                .map_err(|e| Error::Transport(format!("the system D-Bus: {e}")))
        };
        let conn = bus_with(CALL_TIMEOUT)?;
        let connecting = bus_with(CONNECT_WAIT)?;
        let (tx, rx) = mpsc::channel();
        listen(&conn, tx)?;
        let bus = Rc::new(Bus {
            conn,
            connecting,
            verbose,
            signals: rx,
        });
        let mut adapters: Vec<(String, Props)> = bus
            .objects()?
            .into_iter()
            .filter_map(|(path, mut i)| Some((path.to_string(), i.remove(ADAPTER)?)))
            .collect();
        adapters.sort_by(|a, b| a.0.cmp(&b.0));
        bus.say(1, || {
            let names: Vec<&str> = adapters.iter().map(|a| a.0.as_str()).collect();
            format!("adapters {names:?}")
        });
        let (path, props) = adapters
            .into_iter()
            .find(|(p, _)| adapter.is_none_or(|a| p.rsplit('/').next() == Some(a)))
            .ok_or_else(|| {
                Error::Other(match adapter {
                    Some(a) => format!("no Bluetooth adapter {a}"),
                    None => "no Bluetooth adapter".into(),
                })
            })?;
        bus.say(1, || {
            format!(
                "adapter {path}: address {:?}, powered {:?}, discovering {:?}",
                string(&props, "Address"),
                boolean(&props, "Powered"),
                boolean(&props, "Discovering")
            )
        });
        if boolean(&props, "Powered") != Some(true) {
            return Err(Error::Other(format!(
                "Bluetooth is off on {path}: turn it on (bluetoothctl power on)"
            )));
        }
        Ok(Bluez { bus, adapter: path })
    }

    fn mine(&self, path: &str) -> bool {
        path.starts_with(&format!("{}/", self.adapter))
    }

    /// The device object BlueZ holds for `address`, with its properties.
    fn device(&self, address: &str) -> Result<Option<(String, Props)>> {
        Ok(self.bus.objects()?.into_iter().find_map(|(path, mut i)| {
            let p = i.remove(DEVICE)?;
            (self.mine(path.as_str()) && string(&p, "Address").as_deref() == Some(address))
                .then(|| (path.to_string(), p))
        }))
    }

    fn stop_discovery(&self) {
        // Errors are logged; the scan has what it heard either way.
        let _ = self
            .bus
            .call(&self.adapter, ADAPTER, "StopDiscovery", &(), "");
    }

    /// Ends what an attempt to connect left: BlueZ drops a connection it
    /// is still making when told to disconnect, and forgets a device
    /// without a bond when it is removed, so that the next scan hears it
    /// afresh.
    fn cancel(&self, device: &str, paired: bool) {
        let _ = self.bus.call(device, DEVICE, "Disconnect", &(), "");
        if !paired {
            let _ = self.bus.remove(&self.adapter, device);
        }
    }

    /// Waits until the device's services are resolved, which BlueZ says
    /// once it has discovered the attribute table.
    fn resolved(&self, device: &str) -> Result<()> {
        let p = self.bus.properties(device, DEVICE)?;
        if boolean(&p, "ServicesResolved") == Some(true) {
            return Ok(());
        }
        let deadline = Instant::now() + RESOLVE_WAIT;
        while let Some(s) = self.bus.next(deadline) {
            let Signal::Changed(path, iface, p) = s else {
                continue;
            };
            if path.as_str() != device || iface != DEVICE {
                continue;
            }
            self.bus.say(1, || format!("device changed {}", shown(&p)));
            if boolean(&p, "Connected") == Some(false) {
                return Err(Error::Ended);
            }
            if boolean(&p, "ServicesResolved") == Some(true) {
                return Ok(());
            }
        }
        Err(Error::TimedOut(format!(
            "the node's services were not found within {} s",
            RESOLVE_WAIT.as_secs()
        )))
    }

    /// Connects by the device's own method; where BlueZ chose the wrong
    /// bearer for it, BR/EDR, which a node has none of (its advertisement's
    /// flags say so), by the adapter's, which takes the address's type.
    /// That one refuses an address BlueZ has an object for, so the object
    /// goes first, unless it holds a bond.
    fn link_up(&self, device: &str, address: &str, paired: bool) -> Result<String> {
        let c = &self.bus.connecting;
        match self.bus.call_on(c, device, DEVICE, "Connect", &(), "") {
            Ok(_) => return Ok(device.to_string()),
            Err(Error::Transport(e)) if e.contains("br-connection") && !paired => {
                self.bus
                    .say(1, || "BlueZ tried BR/EDR; asking the adapter for LE".into());
            }
            Err(Error::TimedOut(_)) => {
                return Err(Error::TimedOut(format!(
                    "no connection within {:.1} s",
                    CONNECT_WAIT.as_secs_f64()
                )))
            }
            Err(e) => return Err(e),
        }
        self.bus.remove(&self.adapter, device)?;
        let mut to: HashMap<&str, Value> = HashMap::new();
        to.insert("Address", Value::from(address));
        to.insert("AddressType", Value::from("random"));
        let shown = format!("Address={address}, AddressType=random");
        match self
            .bus
            .call_on(c, &self.adapter, ADAPTER, "ConnectDevice", &(to,), &shown)
        {
            Ok(m) => {
                let path: OwnedObjectPath = m
                    .body()
                    .deserialize()
                    .map_err(|e| Error::Transport(e.to_string()))?;
                Ok(path.to_string())
            }
            Err(Error::Transport(e)) if e.contains("UnknownMethod") => Err(Error::Other(
                "BlueZ connected over BR/EDR and has no ConnectDevice: \
                 it needs bluetoothd's experimental interfaces (-E)"
                    .into(),
            )),
            Err(e) => Err(e),
        }
    }

    /// The link to `device`, up and its table read.
    fn open(&self, device: &str, address: &str, paired: bool) -> Result<BluezLink> {
        let device = self.link_up(device, address, paired)?;
        self.resolved(&device)?;
        let mut chars = HashMap::new();
        let mut service = None;
        let objects = self.bus.objects()?;
        for (path, i) in &objects {
            if let Some(p) = i.get(SERVICE) {
                if path.as_str().starts_with(&format!("{device}/")) {
                    self.bus
                        .say(1, || format!("service {path}: {:?}", string(p, "UUID")));
                    if string(p, "UUID") == Some(uuid_text(bark_link::uuid::SERVICE)) {
                        service = Some(path.clone());
                    }
                }
            }
        }
        let service =
            service.ok_or_else(|| Error::Other(format!("{address} offers no bark service")))?;
        let mut mtu = None;
        for (path, i) in &objects {
            let Some(p) = i.get(CHARACTERISTIC) else {
                continue;
            };
            if path_of(p, "Service").as_ref() != Some(&service) {
                continue;
            }
            let uuid = string(p, "UUID").unwrap_or_default();
            if let Some(c) = Char::ALL.into_iter().find(|c| uuid_text(c.uuid()) == uuid) {
                chars.insert(c, path.to_string());
                mtu = mtu.or(uint16(p, "MTU"));
            }
        }
        self.bus.say(1, || {
            format!(
                "{} of {} characteristics found; MTU {mtu:?}",
                chars.len(),
                Char::ALL.len()
            )
        });
        Ok(BluezLink {
            bus: self.bus.clone(),
            adapter: self.adapter.clone(),
            device,
            chars,
            closed: false,
            // BlueZ exchanges the MTU itself as the link comes up, asking
            // for its most, which the node meets with the 247 it offers
            // (spec "Phone link", The service). Before BlueZ said it, the
            // default.
            mtu: mtu.unwrap_or(bark_link::DEFAULT_ATT_MTU),
        })
    }
}

/// Forwards BlueZ's signals to `tx`, from threads of their own.
fn listen(conn: &Connection, tx: mpsc::Sender<Signal>) -> Result<()> {
    let fail = |e: zbus::Error| Error::Transport(format!("subscribing to BlueZ's signals: {e}"));
    let added = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(OBJECTS)
        .and_then(|b| b.member("InterfacesAdded"))
        .map_err(fail)?
        .build();
    let changed = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(PROPERTIES)
        .and_then(|b| b.member("PropertiesChanged"))
        .and_then(|b| b.path_namespace("/org/bluez"))
        .map_err(fail)?
        .build();
    for rule in [added, changed] {
        let it = MessageIterator::for_match_rule(rule, conn, Some(1024)).map_err(fail)?;
        let tx = tx.clone();
        std::thread::spawn(move || {
            for m in it.flatten() {
                let h = m.header();
                let Some(member) = h.member() else { continue };
                let signal = match member.as_str() {
                    "InterfacesAdded" => m
                        .body()
                        .deserialize::<(OwnedObjectPath, Interfaces)>()
                        .ok()
                        .map(|(p, i)| Signal::Added(p, i)),
                    _ => {
                        let path = h.path().map(|p| OwnedObjectPath::from(p.to_owned()));
                        match (path, m.body().deserialize::<(String, Props, Vec<String>)>()) {
                            (Some(p), Ok((iface, props, _))) => {
                                Some(Signal::Changed(p, iface, props))
                            }
                            _ => None,
                        }
                    }
                };
                if let Some(s) = signal {
                    if tx.send(s).is_err() {
                        return;
                    }
                }
            }
        });
    }
    Ok(())
}

/// Properties as the log shows them.
fn shown(p: &Props) -> String {
    let mut keys: Vec<_> = p.iter().collect();
    keys.sort_by(|a, b| a.0.cmp(b.0));
    keys.iter()
        .map(|(k, v)| format!("{k}={v:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Transport for Bluez {
    type Link = BluezLink;

    fn scan(
        &mut self,
        time: Duration,
        enough: &mut dyn FnMut(&[Found], Duration) -> bool,
    ) -> Result<Vec<Found>> {
        // LE alone, and only what advertises the service or the public
        // node's UUID, as the app scans (spec "Phone link", Finding the
        // node); every advertisement, so that RSSI is the latest.
        let uuids = vec![
            uuid_text(bark_link::uuid::SERVICE),
            uuid_text(bark_link::uuid::PUBLIC_NODE),
        ];
        let mut filter: HashMap<&str, Value> = HashMap::new();
        filter.insert("Transport", Value::from("le"));
        filter.insert("UUIDs", Value::from(uuids.clone()));
        filter.insert("DuplicateData", Value::from(true));
        let shown_filter = format!("Transport=le, UUIDs={uuids:?}, DuplicateData=true");
        self.bus.call(
            &self.adapter,
            ADAPTER,
            "SetDiscoveryFilter",
            &(filter,),
            &shown_filter,
        )?;
        // A node's device BlueZ kept from before holds what earlier links
        // added to it and may still be connecting; one without a bond goes,
        // so that what the scan says of it is its advertisement's.
        let mut devices: HashMap<OwnedObjectPath, Device> = HashMap::new();
        for (path, mut i) in self.bus.objects()? {
            let Some(p) = i.remove(DEVICE) else { continue };
            if !self.mine(path.as_str()) {
                continue;
            }
            let mut d = Device::default();
            d.update(&p);
            if d.found().is_some() && !d.paired {
                self.bus.say(1, || {
                    format!("forgetting {}, kept without a bond", d.address)
                });
                if self.bus.remove(&self.adapter, path.as_str()).is_ok() {
                    continue;
                }
            }
            // An RSSI from before the scan is not this scan's.
            d.rssi = None;
            devices.insert(path, d);
        }
        self.bus
            .call(&self.adapter, ADAPTER, "StartDiscovery", &(), "")?;
        let start = Instant::now();
        let deadline = start + time;
        let heard = |devices: &HashMap<OwnedObjectPath, Device>| -> Vec<Found> {
            let mut f: Vec<Found> = devices
                .values()
                .filter(|d| d.heard)
                .filter_map(Device::found)
                .collect();
            f.sort_by(|a, b| a.address.cmp(&b.address));
            f
        };
        let mut done = false;
        while !done && Instant::now() < deadline {
            let tick = deadline.min(Instant::now() + SCAN_TICK);
            if let Some(s) = self.bus.next(tick) {
                let (path, p) = match s {
                    Signal::Added(path, mut i) => match i.remove(DEVICE) {
                        Some(p) => (path, p),
                        None => continue,
                    },
                    Signal::Changed(path, iface, p) if iface == DEVICE => (path, p),
                    Signal::Changed(..) => continue,
                };
                if !self.mine(path.as_str()) {
                    continue;
                }
                let d = devices.entry(path.clone()).or_default();
                d.update(&p);
                if p.contains_key("RSSI") {
                    d.heard = true;
                }
                let level = if d.found().is_some() { 1 } else { 2 };
                self.bus
                    .say(level, || format!("device {path}: {}", shown(&p)));
            }
            done = enough(&heard(&devices), start.elapsed());
        }
        // What BlueZ holds now: a device heard at the same RSSI as before
        // the scan sends no change, but its RSSI is set while discovery
        // runs.
        if !done {
            for (path, mut i) in self.bus.objects()? {
                if let (Some(p), Some(d)) = (i.remove(DEVICE), devices.get_mut(&path)) {
                    if int16(&p, "RSSI").is_some() {
                        d.update(&p);
                        d.heard = true;
                    }
                }
            }
        }
        self.stop_discovery();
        let others = devices
            .values()
            .filter(|d| d.heard && d.found().is_none())
            .count();
        self.bus.say(1, || {
            format!("{others} other devices heard (-vv lists them)")
        });
        Ok(heard(&devices))
    }

    fn connect(&mut self, node: &Found) -> Result<BluezLink> {
        let (device, before) = self
            .device(&node.address)?
            .ok_or_else(|| Error::Other(format!("BlueZ holds no device {}", node.address)))?;
        self.bus
            .say(1, || format!("device {device}: {}", shown(&before)));
        let paired = boolean(&before, "Paired") == Some(true);
        let link = self.open(&device, &node.address, paired);
        if link.is_err() {
            self.cancel(&device, paired);
        }
        link
    }

    fn bonded(&mut self, address: &str) -> Result<bool> {
        Ok(self
            .device(address)?
            .is_some_and(|(_, p)| boolean(&p, "Paired") == Some(true)))
    }

    fn forget(&mut self, address: &str) -> Result<()> {
        match self.device(address)? {
            Some((device, _)) => self.bus.remove(&self.adapter, &device),
            None => Ok(()),
        }
    }
}

pub struct BluezLink {
    bus: Rc<Bus>,
    adapter: String,
    device: String,
    chars: HashMap<Char, String>,
    mtu: u16,
    /// Disconnected already, by [`Link::disconnect`].
    closed: bool,
}

impl BluezLink {
    fn path(&self, c: Char) -> Result<&str> {
        self.chars
            .get(&c)
            .map(String::as_str)
            .ok_or_else(|| Error::Other(format!("the node has no {c:?} characteristic")))
    }

    fn paired(&self) -> bool {
        self.bus
            .properties(&self.device, DEVICE)
            .is_ok_and(|p| boolean(&p, "Paired") == Some(true))
    }
}

impl Link for BluezLink {
    fn mtu(&self) -> u16 {
        self.mtu
    }

    fn read(&mut self, c: Char) -> Result<Vec<u8>> {
        let path = self.path(c)?;
        let options: HashMap<&str, Value> = HashMap::new();
        let m = self.bus.call(
            path,
            CHARACTERISTIC,
            "ReadValue",
            &(options,),
            &format!("{c:?}"),
        )?;
        let v: Vec<u8> = m
            .body()
            .deserialize()
            .map_err(|e| Error::Transport(e.to_string()))?;
        self.bus.say(1, || format!("read {c:?}: {}", hex(&v)));
        Ok(v)
    }

    fn write(&mut self, c: Char, value: &[u8]) -> Result<()> {
        let path = self.path(c)?;
        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("type", Value::from("request"));
        let shown = format!("{c:?} {}, type=request", hex(value));
        self.bus.call(
            path,
            CHARACTERISTIC,
            "WriteValue",
            &(value, options),
            &shown,
        )?;
        Ok(())
    }

    fn subscribe(&mut self, c: Char) -> Result<()> {
        let path = self.path(c)?;
        self.bus
            .call(path, CHARACTERISTIC, "StartNotify", &(), &format!("{c:?}"))
            .map(|_| ())
    }

    fn notified(&mut self, wait: Duration) -> Result<Option<(Char, Vec<u8>)>> {
        let deadline = Instant::now() + wait;
        while let Some(s) = self.bus.next(deadline) {
            let Signal::Changed(path, iface, p) = s else {
                continue;
            };
            if path.as_str() == self.device
                && iface == DEVICE
                && boolean(&p, "Connected") == Some(false)
            {
                self.bus.say(1, || "the link ended".into());
                return Err(Error::Ended);
            }
            if iface != CHARACTERISTIC {
                continue;
            }
            let Some(v) = bytes(&p, "Value") else {
                continue;
            };
            if let Some((&c, _)) = self
                .chars
                .iter()
                .find(|(_, cp)| cp.as_str() == path.as_str())
            {
                self.bus.say(1, || format!("notified {c:?}: {}", hex(&v)));
                return Ok(Some((c, v)));
            }
        }
        Ok(None)
    }

    fn pair(&mut self, code: Option<u32>) -> Result<()> {
        let agent = Agent {
            code,
            verbose: self.bus.verbose,
        };
        let path = OwnedObjectPath::try_from(AGENT_PATH).expect("a valid path");
        self.bus
            .conn
            .object_server()
            .at(AGENT_PATH, agent)
            .map_err(|e| Error::Transport(format!("serving the pairing agent: {e}")))?;
        // The node shows a passkey and takes none (its DisplayOnly), and
        // this side types it, so the agent is one with a keyboard alone:
        // passkey entry (Vol 3 Part H §2.3.5.1, Table 2.8). BlueZ asks the
        // agent of the one who asks for the pairing.
        let registered = self.bus.call(
            "/org/bluez",
            AGENT_MANAGER,
            "RegisterAgent",
            &(&path, "KeyboardOnly"),
            &format!("{AGENT_PATH}, KeyboardOnly"),
        );
        let paired = registered.and_then(|_| {
            self.bus
                .call(&self.device, DEVICE, "Pair", &(), "")
                .map(|_| ())
        });
        let _ = self.bus.call(
            "/org/bluez",
            AGENT_MANAGER,
            "UnregisterAgent",
            &(&path,),
            AGENT_PATH,
        );
        let _ = self.bus.conn.object_server().remove::<Agent, _>(AGENT_PATH);
        paired?;
        let p = self.bus.properties(&self.device, DEVICE)?;
        self.bus
            .say(1, || format!("device after pairing: {}", shown(&p)));
        // A pairing that does not bond the node takes as none.
        if boolean(&p, "Bonded") == Some(false) {
            return Err(Error::Other("paired but not bonded".into()));
        }
        Ok(())
    }

    fn disconnect(mut self) -> Result<()> {
        self.close()
    }
}

/// BlueZ keeps a link up after the program that made it has gone, so a
/// link dropped on an error is ended too.
impl Drop for BluezLink {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close();
        }
    }
}

impl BluezLink {
    fn close(&mut self) -> Result<()> {
        self.closed = true;
        let paired = self.paired();
        let done = self
            .bus
            .call(&self.device, DEVICE, "Disconnect", &(), "")
            .map(|_| ());
        // A guest's device goes, so that the next scan hears what the node
        // advertises then and not what this link added (see [`Device::found`]).
        if !paired {
            let _ = self.bus.remove(&self.adapter, &self.device);
        }
        done
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// BlueZ's answers to an agent's refusal.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    Rejected(String),
}

/// The pairing agent: answers BlueZ's request for the passkey with the
/// code given, or asks the person on the terminal, and refuses every other
/// kind of pairing, as the node offers none (spec "Phone link", Pairing).
struct Agent {
    code: Option<u32>,
    verbose: u8,
}

impl Agent {
    fn say(&self, s: &str) {
        if self.verbose >= 1 {
            eprintln!("agent: {s}");
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {
        self.say("Release");
    }

    async fn request_passkey(
        &self,
        device: OwnedObjectPath,
    ) -> std::result::Result<u32, AgentError> {
        self.say(&format!("RequestPasskey {device}"));
        let code = match self.code {
            Some(c) => Some(c),
            None => {
                blocking::unblock(|| {
                    eprint!("The code on the node's screen: ");
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line).ok()?;
                    line.trim().parse::<u32>().ok()
                })
                .await
            }
        };
        match code {
            Some(c) if c < bark_link::phones::CODES => {
                self.say(&format!("answering passkey {c:06}"));
                Ok(c)
            }
            _ => Err(AgentError::Rejected("not a six-digit code".into())),
        }
    }

    fn request_pin_code(&self, device: OwnedObjectPath) -> std::result::Result<String, AgentError> {
        self.say(&format!("RequestPinCode {device}: refused"));
        Err(AgentError::Rejected("the node pairs by passkey".into()))
    }

    fn display_pin_code(
        &self,
        device: OwnedObjectPath,
        pincode: String,
    ) -> std::result::Result<(), AgentError> {
        self.say(&format!("DisplayPinCode {device} {pincode}: refused"));
        Err(AgentError::Rejected("the node pairs by passkey".into()))
    }

    fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, entered: u16) {
        self.say(&format!(
            "DisplayPasskey {device} {passkey:06} entered {entered}"
        ));
    }

    fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> std::result::Result<(), AgentError> {
        self.say(&format!(
            "RequestConfirmation {device} {passkey:06}: refused"
        ));
        Err(AgentError::Rejected(
            "the node pairs by passkey entry".into(),
        ))
    }

    fn request_authorization(
        &self,
        device: OwnedObjectPath,
    ) -> std::result::Result<(), AgentError> {
        self.say(&format!("RequestAuthorization {device}: refused"));
        Err(AgentError::Rejected("the node pairs by passkey".into()))
    }

    fn authorize_service(
        &self,
        device: OwnedObjectPath,
        uuid: String,
    ) -> std::result::Result<(), AgentError> {
        self.say(&format!("AuthorizeService {device} {uuid}: refused"));
        Err(AgentError::Rejected("no service is offered".into()))
    }

    fn cancel(&self) {
        self.say("Cancel: BlueZ ended the request");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(kv: &[(&str, Value<'_>)]) -> Props {
        kv.iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    OwnedValue::try_from(v.try_clone().unwrap()).unwrap(),
                )
            })
            .collect()
    }

    /// Seen public, linked, seen again: the link adds the service's UUID
    /// to the device, and the node stays public.
    #[test]
    fn a_node_linked_once_stays_public() {
        let mut d = Device::default();
        d.update(&props(&[
            ("Address", Value::from("E2:49:1C:03:EF:88")),
            ("Name", Value::from("bark-bae6e6cbf4f5")),
            ("RSSI", Value::from(-53i16)),
            (
                "UUIDs",
                Value::from(vec![uuid_text(bark_link::uuid::PUBLIC_NODE)]),
            ),
        ]));
        assert!(d.found().unwrap().public);
        d.update(&props(&[(
            "UUIDs",
            Value::from(vec![
                uuid_text(bark_link::uuid::PUBLIC_NODE),
                uuid_text(bark_link::uuid::SERVICE),
            ]),
        )]));
        let f = d.found().unwrap();
        assert!(f.public);
        assert_eq!(crate::link::id_text(f.id), "bae6e6cbf4f5");
    }

    #[test]
    fn a_private_node_advertises_the_service_alone() {
        let mut d = Device::default();
        d.update(&props(&[
            ("Name", Value::from("bark-bae6e6cbf4f5")),
            (
                "UUIDs",
                Value::from(vec![uuid_text(bark_link::uuid::SERVICE)]),
            ),
        ]));
        assert!(!d.found().unwrap().public);
    }
}
