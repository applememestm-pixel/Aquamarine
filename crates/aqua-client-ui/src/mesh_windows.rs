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

use super::{book, run_session};

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
    std::thread::spawn(move || match serve(app2) {
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

/// Devices to try: paired ones, plus (on a manual search) those in range.
fn candidates(scan: bool) -> windows::core::Result<Vec<(u64, BluetoothDevice)>> {
    let mut out = Vec::new();
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

fn dial(dev: &BluetoothDevice) -> windows::core::Result<Option<StreamSocket>> {
    let sid = RfcommServiceId::FromUuid(SERVICE)?;
    let res = dev.GetRfcommServicesForIdWithCacheModeAsync(&sid, BluetoothCacheMode::Uncached)?.join()?;
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
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut devs = candidates(scan).map_err(err)?;
        devs.sort_by_key(|(a, _)| std::cmp::Reverse(known.get(&mac(*a)).copied().unwrap_or(0)));
        devs.dedup_by_key(|(a, _)| *a);
        let mut met = 0;
        for (addr, dev) in devs.into_iter().take(10) {
            let Ok(Some(sock)) = dial(&dev) else { continue };
            let Ok(mut link) = Link::new(sock) else { continue };
            if run_session(&app2, &mut link, true, &mac(addr)).error.is_none() {
                met += 1;
            }
        }
        if met == 0 && scan {
            return Err("no contacts with Aquamarine answered nearby".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
