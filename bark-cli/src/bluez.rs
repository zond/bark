//! The transport on Linux: BlueZ, the Linux Bluetooth daemon, driven over
//! the system D-Bus (BlueZ's `doc/org.bluez.*.rst`). Every call and every
//! error BlueZ answers is logged with `-v`, verbatim, so that a link that
//! fails can be read from the log.

use std::collections::HashMap;
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
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECTS: &str = "org.freedesktop.DBus.ObjectManager";

/// Longer than any one step BlueZ takes: an LE connection it gives up
/// after 20 s by the kernel's default, and a read or write it gives up
/// after the ATT transaction's 30 s (Vol 3 Part F §3.3.3).
const CALL_TIMEOUT: Duration = Duration::from_secs(40);

/// How long the GATT discovery may take after the link is up. BlueZ
/// discovers the whole table before it says the services are resolved;
/// the node's table is three services, a few round trips.
const RESOLVE_WAIT: Duration = Duration::from_secs(30);

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
    verbose: bool,
    signals: Receiver<Signal>,
}

impl Bus {
    fn say(&self, s: impl FnOnce() -> String) {
        if self.verbose {
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
        self.say(|| format!("call {iface}.{method}({shown}) on {path}"));
        match self
            .conn
            .call_method(Some(BLUEZ), path, Some(iface), method, body)
        {
            Ok(m) => Ok(m),
            Err(e) => {
                self.say(|| format!("error from {iface}.{method}: {}", verbatim(&e)));
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
    if let zbus::Error::MethodError(name, text, _) = e {
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
    }
    Error::Transport(verbatim(e))
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

/// A device as the scan has it so far.
#[derive(Default)]
struct Device {
    address: String,
    name: Option<String>,
    rssi: Option<i16>,
    uuids: Vec<String>,
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
        for u in strings(p, "UUIDs") {
            if !self.uuids.contains(&u) {
                self.uuids.push(u);
            }
        }
    }

    /// The node it is, if it is one: named as a node, and public unless
    /// it advertised the service's UUID, as a private node does
    /// (spec "Phone link", Public).
    fn found(&self) -> Option<Found> {
        Some(Found {
            id: id_of_name(self.name.as_deref()?)?,
            address: self.address.clone(),
            rssi: self.rssi,
            public: !self.uuids.contains(&uuid_text(bark_link::uuid::SERVICE)),
        })
    }
}

pub struct Bluez {
    bus: Rc<Bus>,
    adapter: String,
    /// The D-Bus object of each device a scan found, by address.
    devices: HashMap<String, OwnedObjectPath>,
}

impl Bluez {
    /// On the system bus, with the adapter named `adapter`, such as
    /// `hci0`, or else the first BlueZ lists.
    pub fn new(adapter: Option<&str>, verbose: bool) -> Result<Bluez> {
        let conn = zbus::blocking::connection::Builder::system()
            .and_then(|b| b.method_timeout(CALL_TIMEOUT).build())
            .map_err(|e| Error::Transport(format!("the system D-Bus: {e}")))?;
        let (tx, rx) = mpsc::channel();
        listen(&conn, tx)?;
        let bus = Rc::new(Bus {
            conn,
            verbose,
            signals: rx,
        });
        let mut adapters: Vec<(String, Props)> = bus
            .objects()?
            .into_iter()
            .filter_map(|(path, mut i)| Some((path.to_string(), i.remove(ADAPTER)?)))
            .collect();
        adapters.sort_by(|a, b| a.0.cmp(&b.0));
        bus.say(|| {
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
        bus.say(|| {
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
        Ok(Bluez {
            bus,
            adapter: path,
            devices: HashMap::new(),
        })
    }

    fn stop_discovery(&self) {
        // Errors are logged; the scan has what it heard either way.
        let _ = self
            .bus
            .call(&self.adapter, ADAPTER, "StopDiscovery", &(), "");
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
            self.bus.say(|| format!("device changed {}", shown(&p)));
            if boolean(&p, "Connected") == Some(false) {
                return Err(Error::Ended);
            }
            if boolean(&p, "ServicesResolved") == Some(true) {
                return Ok(());
            }
        }
        Err(Error::Other(format!(
            "the node's services were not resolved within {} s",
            RESOLVE_WAIT.as_secs()
        )))
    }

    /// Connects by the device's own method; where BlueZ chose the wrong
    /// bearer for it, BR/EDR, which a node has none of (its advertisement's
    /// flags say so), by the adapter's, which takes the address's type.
    /// That one refuses an address BlueZ has an object for, so the object
    /// goes first, unless it holds a bond.
    fn link_up(&self, device: &str, address: &str, paired: bool) -> Result<String> {
        match self.bus.call(device, DEVICE, "Connect", &(), "") {
            Ok(_) => return Ok(device.to_string()),
            Err(Error::Transport(e)) if e.contains("br-connection") && !paired => {
                self.bus
                    .say(|| "BlueZ tried BR/EDR; asking the adapter for LE".into());
            }
            Err(e) => return Err(e),
        }
        let path = OwnedObjectPath::try_from(device.to_string())
            .map_err(|e| Error::Transport(e.to_string()))?;
        self.bus
            .call(&self.adapter, ADAPTER, "RemoveDevice", &(path,), device)?;
        let mut to: HashMap<&str, Value> = HashMap::new();
        to.insert("Address", Value::from(address));
        to.insert("AddressType", Value::from("random"));
        let shown = format!("Address={address}, AddressType=random");
        match self
            .bus
            .call(&self.adapter, ADAPTER, "ConnectDevice", &(to,), &shown)
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
        enough: &mut dyn FnMut(&[Found]) -> bool,
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
        let mut devices: HashMap<OwnedObjectPath, Device> = HashMap::new();
        for (path, mut i) in self.bus.objects()? {
            if let Some(p) = i.remove(DEVICE) {
                if !path.as_str().starts_with(&format!("{}/", self.adapter)) {
                    continue;
                }
                let mut d = Device::default();
                d.update(&p);
                // An RSSI from before the scan is not this scan's.
                d.rssi = None;
                devices.insert(path, d);
            }
        }
        self.bus
            .call(&self.adapter, ADAPTER, "StartDiscovery", &(), "")?;
        let deadline = Instant::now() + time;
        let mut nodes = Vec::new();
        let mut done = false;
        while !done {
            let Some(s) = self.bus.next(deadline) else {
                break;
            };
            let (path, p) = match s {
                Signal::Added(path, mut i) => match i.remove(DEVICE) {
                    Some(p) => (path, p),
                    None => continue,
                },
                Signal::Changed(path, iface, p) if iface == DEVICE => (path, p),
                Signal::Changed(..) => continue,
            };
            if !path.as_str().starts_with(&format!("{}/", self.adapter)) {
                continue;
            }
            let d = devices.entry(path.clone()).or_default();
            d.update(&p);
            if p.contains_key("RSSI") {
                d.heard = true;
            }
            self.bus.say(|| format!("device {path}: {}", shown(&p)));
            let heard: Vec<Found> = devices
                .values()
                .filter(|d| d.heard)
                .filter_map(Device::found)
                .collect();
            if heard != nodes {
                nodes = heard;
                done = enough(&nodes);
            }
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
        let mut found = Vec::new();
        for (path, d) in &devices {
            if !d.heard {
                continue;
            }
            match d.found() {
                Some(f) => {
                    self.devices.insert(f.address.clone(), path.clone());
                    found.push(f);
                }
                None => self.bus.say(|| {
                    format!(
                        "{} at {path} is not named as a node: {:?}",
                        d.address, d.name
                    )
                }),
            }
        }
        Ok(found)
    }

    fn connect(&mut self, node: &Found) -> Result<BluezLink> {
        let device = self
            .devices
            .get(&node.address)
            .ok_or_else(|| Error::Other(format!("{} was not heard in a scan", node.address)))?
            .to_string();
        let before = self.bus.properties(&device, DEVICE)?;
        self.bus
            .say(|| format!("device {device}: {}", shown(&before)));
        let paired = boolean(&before, "Paired") == Some(true);
        let device = self.link_up(&device, &node.address, paired)?;
        self.resolved(&device)?;
        let mut chars = HashMap::new();
        let mut service = None;
        let objects = self.bus.objects()?;
        for (path, i) in &objects {
            if let Some(p) = i.get(SERVICE) {
                if path.as_str().starts_with(&format!("{device}/")) {
                    self.bus
                        .say(|| format!("service {path}: {:?}", string(p, "UUID")));
                    if string(p, "UUID") == Some(uuid_text(bark_link::uuid::SERVICE)) {
                        service = Some(path.clone());
                    }
                }
            }
        }
        let service = service
            .ok_or_else(|| Error::Other(format!("{} offers no bark service", node.address)))?;
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
        self.bus.say(|| {
            format!(
                "{} of {} characteristics found; MTU {mtu:?}",
                chars.len(),
                Char::ALL.len()
            )
        });
        Ok(BluezLink {
            bus: self.bus.clone(),
            device,
            chars,
            // BlueZ exchanges the MTU itself as the link comes up, asking
            // for its most, which the node meets with the 247 it offers
            // (spec "Phone link", The service). Before BlueZ said it, the
            // default.
            mtu: mtu.unwrap_or(bark_link::DEFAULT_ATT_MTU),
        })
    }
}

pub struct BluezLink {
    bus: Rc<Bus>,
    device: String,
    chars: HashMap<Char, String>,
    mtu: u16,
}

impl BluezLink {
    fn path(&self, c: Char) -> Result<&str> {
        self.chars
            .get(&c)
            .map(String::as_str)
            .ok_or_else(|| Error::Other(format!("the node has no {c:?} characteristic")))
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
        self.bus.say(|| format!("read {c:?}: {}", hex(&v)));
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

    fn disconnect(self) -> Result<()> {
        self.bus
            .call(&self.device, DEVICE, "Disconnect", &(), "")
            .map(|_| ())
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
