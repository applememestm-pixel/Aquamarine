//! Nearby delivery over Bluetooth on Windows 10/11: an RFCOMM service
//! (WinRT `RfcommServiceProvider` + `StreamSocketListener`) with
//! Aquamarine's service UUID; outgoing connections to paired devices (and,
//! on a manual search, to ones in range) that publish it. The session on
//! top is mesh.rs's — contacts prove their identity keys.
//!
//! Everything WinRT lives on plain threads with blocking `.join()`.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::AppHandle;
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::Rfcomm::{RfcommServiceId, RfcommServiceProvider};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::TypedEventHandler;
use windows::Networking::Sockets::{SocketProtectionLevel, StreamSocket, StreamSocketListener, StreamSocketListenerConnectionReceivedEventArgs};
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};

use super::{book, direct_addrs, note_attempt, run_session};

pub const SUPPORTED: bool = true;

static READY: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// a9a1e5c0-7b3d-4f2e-9c61-5e2d8b0f4a17 (super::SERVICE_UUID)
const SERVICE: GUID = GUID::from_u128(0xa9a1e5c0_7b3d_4f2e_9c61_5e2d8b0f4a17);
const PROTECTION: SocketProtectionLevel = SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication;

fn err(e: windows::core::Error) -> String {
    let code = e.code().0 as u32;
    match code {
        // ERROR_DEVICE_NOT_AVAILABLE / radio off
        0x8007_10DF | 0x8007_048F => "Bluetooth is off".into(),
        _ => format!("Bluetooth: {}", e.message()),
    }
}

fn mac(addr: u64) -> String {
    let b = addr.to_be_bytes();
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[2], b[3], b[4], b[5], b[6], b[7])
}

/// The socket as a blocking byte stream.
struct Link {
    sock: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
}

impl Link {
    fn new(sock: StreamSocket) -> windows::core::Result<Self> {
        let reader = DataReader::CreateDataReader(&sock.InputStream()?)?;
        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
        let writer = DataWriter::CreateDataWriter(&sock.OutputStream()?)?;
        Ok(Self { sock, reader, writer })
    }
}

impl Read for Link {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let op = self.reader.LoadAsync(buf.len().min(65536) as u32).map_err(std::io::Error::other)?;
        // A peer that stops answering: give up after 25 s.
        let watch = op.clone();
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let d2 = done.clone();
        std::thread::spawn(move || {
            for _ in 0..250 {
                std::thread::sleep(Duration::from_millis(100));
                if d2.load(Ordering::Relaxed) {
                    return;
                }
            }
            let _ = watch.Cancel();
        });
        let n = op.join();
        done.store(true, Ordering::Relaxed);
        let n = n.map_err(|e| std::io::Error::new(std::io::ErrorKind::TimedOut, e.message().to_string()))? as usize;
        if n == 0 {
            return Ok(0);
        }
        let n = n.min(buf.len());
        self.reader.ReadBytes(&mut buf[..n]).map_err(std::io::Error::other)?;
        Ok(n)
    }
}

impl Write for Link {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.writer.WriteBytes(data).map_err(std::io::Error::other)?;
        self.writer.StoreAsync().map_err(std::io::Error::other)?.join().map_err(std::io::Error::other)?;
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.FlushAsync().map_err(std::io::Error::other)?.join().map_err(std::io::Error::other)?;
        Ok(())
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.sock.Close();
    }
}

fn serve(app: AppHandle) -> windows::core::Result<(RfcommServiceProvider, StreamSocketListener)> {
    let sid = RfcommServiceId::FromUuid(SERVICE)?;
    let provider = RfcommServiceProvider::CreateAsync(&sid)?.join()?;
    let listener = StreamSocketListener::new()?;
    listener.ConnectionReceived(&TypedEventHandler::<StreamSocketListener, StreamSocketListenerConnectionReceivedEventArgs>::new(
        move |_, args| {
            if let Some(args) = args.as_ref() {
                let sock = args.Socket()?;
                let who = sock.Information().and_then(|i| i.RemoteHostName()).and_then(|h| h.CanonicalName()).map(|s| s.to_string()).unwrap_or_default();
                let app = app.clone();
                std::thread::spawn(move || {
                    if let Ok(mut link) = Link::new(sock) {
                        run_session(&app, &mut link, false, &who.trim_matches(|c| c == '(' || c == ')').to_uppercase());
                    }
                });
            }
            Ok(())
        },
    ))?;
    listener.BindServiceNameWithProtectionLevelAsync(&sid.AsString()?, PROTECTION)?.join()?;
    // SDP: the service name (attribute 0x0100, a text element).
    let w = DataWriter::new()?;
    let name = "Aquamarine";
    w.WriteByte(0x25)?; // text string, 8-bit length
    w.WriteByte(name.len() as u8)?;
    w.WriteString(&HSTRING::from(name))?;
    provider.SdpRawAttributes()?.Insert(0x100, &w.DetachBuffer()?)?;
    provider.StartAdvertisingWithRadioDiscoverability(&listener, true)?;
    Ok((provider, listener))
}

pub async fn listen(app: &AppHandle) -> Result<(), String> {
    if ready() {
        return Ok(());
    }
    STOP.store(false, Ordering::Relaxed);
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let app2 = app.clone();
    // The provider and listener live on this thread for as long as we listen.
    std::thread::spawn(move || match { read_own_address(); serve(app2) } {
        Ok((provider, listener)) => {
            let _ = tx.send(Ok(()));
            while !STOP.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
            }
            let _ = provider.StopAdvertising();
            let _ = listener.Close();
        }
        Err(e) => {
            let _ = tx.send(Err(err(e)));
        }
    });
    let r = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(20)).unwrap_or(Err("Bluetooth didn't answer".into())))
        .await
        .map_err(|e| e.to_string())?;
    if r.is_ok() {
        READY.store(true, Ordering::Relaxed);
        tracing::info!("mesh: listening for nearby contacts over Bluetooth");
    }
    r
}

pub async fn stop(_app: &AppHandle) {
    STOP.store(true, Ordering::Relaxed);
    READY.store(false, Ordering::Relaxed);
}

/// This PC's Bluetooth address (for my QR), read on the Bluetooth thread
/// when listening starts — never on the UI thread.
static OWN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
pub fn own_address() -> Option<String> {
    OWN.lock().ok().and_then(|g| g.clone())
}
fn read_own_address() {
    let a = windows::Devices::Bluetooth::BluetoothAdapter::GetDefaultAsync().and_then(|op| op.join()).and_then(|ad| ad.BluetoothAddress());
    if let (Ok(a), Ok(mut g)) = (a, OWN.lock()) {
        *g = Some(mac(a));
    }
}

fn parse_mac(m: &str) -> Option<u64> {
    let hex: String = m.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (hex.len() == 12).then(|| u64::from_str_radix(&hex, 16).ok()).flatten()
}

/// Devices to try: ones from QR codes and ones we've met (dialled directly,
/// no pairing — like Briar), paired ones, and (on a manual search) those in range.
fn candidates(scan: bool, direct: &[String]) -> windows::core::Result<Vec<(u64, BluetoothDevice)>> {
    let mut out = Vec::new();
    for m in direct {
        let Some(a) = parse_mac(m) else { continue };
        if let Ok(dev) = BluetoothDevice::FromBluetoothAddressAsync(a).and_then(|op| op.join()) {
            out.push((a, dev));
        }
    }
    let mut sels = vec![BluetoothDevice::GetDeviceSelectorFromPairingState(true)?];
    if scan {
        sels.push(BluetoothDevice::GetDeviceSelectorFromPairingState(false)?);
    }
    for sel in sels {
        let infos = DeviceInformation::FindAllAsyncAqsFilter(&sel)?.join()?;
        for i in 0..infos.Size()? {
            let info = infos.GetAt(i)?;
            if let Ok(dev) = BluetoothDevice::FromIdAsync(&info.Id()?)?.join() {
                if let Ok(a) = dev.BluetoothAddress() {
                    out.push((a, dev));
                }
            }
        }
    }
    Ok(out)
}

/// Why a device gave nothing: the service lookup's own error (out of range,
/// access denied, not paired…) or "no Aquamarine there".
fn dial(dev: &BluetoothDevice) -> Result<StreamSocket, String> {
    dial_inner(dev).map_err(err)?.ok_or_else(|| "Aquamarine isn't open there (or Bluetooth delivery is off on it)".to_string())
}

fn dial_inner(dev: &BluetoothDevice) -> windows::core::Result<Option<StreamSocket>> {
    let sid = RfcommServiceId::FromUuid(SERVICE)?;
    let res = dev.GetRfcommServicesForIdWithCacheModeAsync(&sid, BluetoothCacheMode::Uncached)?.join()?;
    // Windows says why it found nothing (the device is away, access denied).
    let code = res.Error()?;
    if code != windows::Devices::Bluetooth::BluetoothError::Success {
        let why = match code {
            windows::Devices::Bluetooth::BluetoothError::RadioNotAvailable => "Bluetooth is off",
            windows::Devices::Bluetooth::BluetoothError::DeviceNotConnected => "the device is out of range or asleep",
            windows::Devices::Bluetooth::BluetoothError::ConsentRequired => "Windows asks for permission — allow Bluetooth for the app",
            windows::Devices::Bluetooth::BluetoothError::NotSupported => "this device has no Bluetooth Classic",
            _ => "the device didn't answer",
        };
        return Err(windows::core::Error::new(windows::core::HRESULT(0x8000_4005u32 as i32), why));
    }
    let services = res.Services()?;
    if services.Size()? == 0 {
        return Ok(None);
    }
    let svc = services.GetAt(0)?;
    let sock = StreamSocket::new()?;
    sock.ConnectWithProtectionLevelAsync(&svc.ConnectionHostName()?, &svc.ConnectionServiceName()?, PROTECTION)?.join()?;
    Ok(Some(sock))
}

pub async fn sync_round(app: &AppHandle, scan: bool) -> Result<(), String> {
    listen(app).await?;
    let known: std::collections::HashMap<String, u64> = book(app).peers.iter().map(|(k, p)| (k.clone(), p.last)).collect();
    let direct = direct_addrs(app);
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut devs = candidates(scan, &direct).map_err(err)?;
        // From a QR first, then the ones met most recently.
        devs.sort_by_key(|(a, _)| (!direct.contains(&mac(*a)), std::cmp::Reverse(known.get(&mac(*a)).copied().unwrap_or(0))));
        let mut seen = std::collections::HashSet::new();
        devs.retain(|(a, _)| seen.insert(*a));
        if devs.is_empty() {
            return Err("nobody to dial yet — scan the other device's QR (Contacts → Meet nearby) or pair it once in Windows Settings".to_string());
        }
        let mut met = 0;
        let mut why = String::new();
        for (addr, dev) in devs.into_iter().take(10) {
            let name = dev.Name().map(|n| n.to_string()).unwrap_or_default();
            let label = if name.is_empty() { mac(addr) } else { format!("{} ({name})", mac(addr)) };
            let sock = match dial(&dev) {
                Ok(s) => s,
                Err(e) => {
                    // Only on a search by hand: the background rounds would fill the list.
                    if scan { note_attempt(&app2, &label, &e); }
                    why = e;
                    continue;
                }
            };
            let mut link = match Link::new(sock) {
                Ok(l) => l,
                Err(e) => { why = err(e); if scan { note_attempt(&app2, &label, &why); } continue; }
            };
            if run_session(&app2, &mut link, true, &mac(addr)).error.is_none() {
                met += 1;
            }
        }
        if met == 0 && scan {
            return Err(if why.is_empty() { "no contacts with Aquamarine answered nearby".to_string() } else { why });
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
