package app.aquamarine.messenger

// Nearby delivery over Bluetooth (the Briar way) — the Android half.
// Copied into the generated project next to MainActivity by CI and
// android/patch-project.sh; driven from Rust (src/mesh_android.rs).
//
// Plain RFCOMM byte pipes: listen / accept / connect / read / write /
// close. Insecure RFCOMM, so phones don't have to be paired — the two
// apps authenticate each other with their identity keys on top.
// Everything that blocks runs on a worker thread; a command answers when
// its work is done.

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.IOException
import java.net.Inet4Address
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

@InvokeArg
class MeshListenArgs {
  var uuid: String = ""
  var name: String = "Aquamarine"
}

@InvokeArg
class MeshAcceptArgs {
  var timeoutMs: Long = 30000
}

@InvokeArg
class MeshConnectArgs {
  var address: String = ""
  var uuid: String = ""
}

@InvokeArg
class MeshIoArgs {
  var id: Long = 0
  var max: Int = 65536
  var timeoutMs: Long = 25000
  var data: String = ""
}

@InvokeArg
class MeshDevicesArgs {
  var scanSecs: Int = 0
}

@InvokeArg
class MeshLockArgs {
  var on: Boolean = false
}

@TauriPlugin
class AquaMeshPlugin(private val activity: Activity) : Plugin(activity) {
  private val pool = Executors.newCachedThreadPool()
  private val discoveryLock = Any()
  private val conns = ConcurrentHashMap<Long, BluetoothSocket>()
  private val nextId = AtomicLong(1)
  private val accepted = LinkedBlockingQueue<Pair<Long, String>>()
  @Volatile private var server: BluetoothServerSocket? = null
  private var multicast: WifiManager.MulticastLock? = null

  private fun adapter(): BluetoothAdapter? =
    (activity.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

  private fun needed(): Array<String> =
    if (Build.VERSION.SDK_INT >= 31) arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_SCAN)
    else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)

  private fun permitted(): Boolean =
    needed().all { activity.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }

  private fun snapshot(): JSObject {
    val a = adapter()
    val o = JSObject()
    o.put("supported", a != null)
    o.put("enabled", runCatching { a?.isEnabled == true }.getOrDefault(false))
    o.put("permitted", permitted())
    return o
  }

  private fun later(invoke: Invoke, work: () -> JSObject) {
    pool.execute {
      try {
        invoke.resolve(work())
      } catch (e: Exception) {
        invoke.reject(e.message ?: e.toString())
      }
    }
  }

  @Command
  fun state(invoke: Invoke) {
    invoke.resolve(snapshot())
  }

  /** Asks for "Nearby devices" (Android 12+) or location (older), then reports. */
  @Command
  fun ask(invoke: Invoke) {
    val missing = needed().filter { activity.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
    if (missing.isEmpty()) { invoke.resolve(snapshot()); return }
    val host = activity as? MainActivity
    if (host == null) { invoke.reject("Bluetooth permission prompt unavailable"); return }
    host.requestMeshPermissions(missing.toTypedArray()) { invoke.resolve(snapshot()) }
  }

  @SuppressLint("MissingPermission")
  @Command
  fun listen(invoke: Invoke) {
    val args = invoke.parseArgs(MeshListenArgs::class.java)
    later(invoke) {
      if (server == null) {
        val a = adapter() ?: throw IOException("no Bluetooth")
        val s = a.listenUsingInsecureRfcommWithServiceRecord(args.name, UUID.fromString(args.uuid))
        server = s
        Thread {
          var cur = s
          while (server === cur) {
            try {
              val sock = cur.accept() ?: continue
              val id = nextId.getAndIncrement()
              conns[id] = sock
              accepted.put(Pair(id, sock.remoteDevice?.address ?: ""))
            } catch (e: IOException) {
              if (server !== cur) break
              Thread.sleep(2000)
              // Bluetooth was switched off and on: that socket is dead for
              // good (it used to fail every second until the app restarted,
              // and nobody nearby could reach us). Listen again once it's on.
              val now = adapter()
              if (now != null && now.isEnabled) {
                try {
                  val fresh = now.listenUsingInsecureRfcommWithServiceRecord(args.name, UUID.fromString(args.uuid))
                  try { cur.close() } catch (_: IOException) {}
                  if (server === cur) { server = fresh; cur = fresh } else { fresh.close(); break }
                } catch (_: Exception) {}
              }
            }
          }
        }.apply { isDaemon = true; start() }
      }
      JSObject()
    }
  }

  @Command
  fun accept(invoke: Invoke) {
    val args = invoke.parseArgs(MeshAcceptArgs::class.java)
    later(invoke) {
      val o = JSObject()
      val next = accepted.poll(args.timeoutMs, TimeUnit.MILLISECONDS)
      if (next != null) { o.put("id", next.first); o.put("address", next.second) }
      o
    }
  }

  @Command
  fun stop(invoke: Invoke) {
    later(invoke) {
      val s = server
      server = null
      try { s?.close() } catch (_: IOException) {}
      JSObject()
    }
  }

  @SuppressLint("MissingPermission")
  @Command
  fun connect(invoke: Invoke) {
    val args = invoke.parseArgs(MeshConnectArgs::class.java)
    later(invoke) {
      val a = adapter() ?: throw IOException("no Bluetooth")
      try { a.cancelDiscovery() } catch (_: Exception) {}
      val dev = a.getRemoteDevice(args.address)
      val sock = dev.createInsecureRfcommSocketToServiceRecord(UUID.fromString(args.uuid))
      try {
        sock.connect()
      } catch (e: Exception) {
        try { sock.close() } catch (_: IOException) {}
        throw e
      }
      val id = nextId.getAndIncrement()
      conns[id] = sock
      val o = JSObject()
      o.put("id", id)
      o.put("address", args.address)
      o
    }
  }

  @Command
  fun read(invoke: Invoke) {
    val args = invoke.parseArgs(MeshIoArgs::class.java)
    later(invoke) {
      val sock = conns[args.id] ?: throw IOException("closed")
      val input = sock.inputStream
      val o = JSObject()
      // RFCOMM streams have no read timeout: wait for data in small steps.
      var waited = 0L
      while (input.available() == 0 && waited < args.timeoutMs) {
        if (!sock.isConnected) break
        Thread.sleep(20); waited += 20
      }
      if (input.available() == 0 && waited >= args.timeoutMs) {
        o.put("data", "")
        return@later o
      }
      val buf = ByteArray(maxOf(1, minOf(args.max, 262144)))
      val n = input.read(buf)
      if (n < 0) { o.put("eof", true); return@later o }
      o.put("data", Base64.encodeToString(buf, 0, n, Base64.NO_WRAP))
      o
    }
  }

  @Command
  fun write(invoke: Invoke) {
    val args = invoke.parseArgs(MeshIoArgs::class.java)
    later(invoke) {
      val sock = conns[args.id] ?: throw IOException("closed")
      sock.outputStream.write(Base64.decode(args.data, Base64.NO_WRAP))
      sock.outputStream.flush()
      JSObject()
    }
  }

  @Command
  fun close(invoke: Invoke) {
    val args = invoke.parseArgs(MeshIoArgs::class.java)
    later(invoke) {
      conns.remove(args.id)?.let { try { it.close() } catch (_: IOException) {} }
      JSObject()
    }
  }

  /** Make the phone showing the QR discoverable before the other phone scans. */
  @SuppressLint("MissingPermission")
  @Command
  fun visible(invoke: Invoke) {
    val a = adapter()
    val host = activity as? MainActivity
    if (a == null || host == null) { invoke.reject("Bluetooth is unavailable"); return }
    if (a.scanMode == BluetoothAdapter.SCAN_MODE_CONNECTABLE_DISCOVERABLE) {
      invoke.resolve(JSObject())
      return
    }
    val intent = Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE)
      .putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, 120)
    val showPrompt = {
      host.requestMeshVisibility(intent) { allowed ->
        if (allowed) invoke.resolve(JSObject()) else invoke.reject("Bluetooth visibility was not enabled")
      }
    }
    if (Build.VERSION.SDK_INT >= 31 &&
        activity.checkSelfPermission(Manifest.permission.BLUETOOTH_ADVERTISE) != PackageManager.PERMISSION_GRANTED) {
      host.requestMeshPermissions(arrayOf(Manifest.permission.BLUETOOTH_ADVERTISE)) { allowed ->
        if (allowed) showPrompt() else invoke.reject("Bluetooth visibility requires the Nearby devices permission")
      }
    } else {
      showPrompt()
    }
  }

  /** Same Wi-Fi (mesh_lan.rs): many phones drop broadcast packets unless an
   *  app holds a multicast lock — the "who's here?" beacons never arrived. */
  @Command
  fun lanLock(invoke: Invoke) {
    val args = invoke.parseArgs(MeshLockArgs::class.java)
    try {
      if (args.on) {
        if (multicast == null) {
          val wm = activity.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
          multicast = wm.createMulticastLock("aquamarine:nearby").apply { setReferenceCounted(false); acquire() }
        }
      } else {
        multicast?.release()
        multicast = null
      }
    } catch (_: Exception) {}
    invoke.resolve(JSObject())
  }

  /** The Wi-Fi network's handle (0: none), for `android_setsocknetwork` in
   *  mesh_lan.rs. Wi-Fi without internet isn't the default network — mobile
   *  data is — and sockets of the local network went there: neighbours on
   *  the same Wi-Fi were never found. Only those sockets are put on Wi-Fi;
   *  the relay keeps the default network. */
  @Command
  fun wifiHandle(invoke: Invoke) {
    var handle = 0L
    var ipv4: String? = null
    try {
      val cm = activity.applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
      @Suppress("DEPRECATION")
      for (n in cm.allNetworks) {
        val caps = cm.getNetworkCapabilities(n) ?: continue
        if (!caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) continue
        if (handle == 0L) handle = n.networkHandle
        val address = cm.getLinkProperties(n)?.linkAddresses?.map { it.address }
          ?.firstOrNull { it is Inet4Address && !it.isLoopbackAddress }?.hostAddress
        if (address != null) {
          handle = n.networkHandle
          ipv4 = address
          break
        }
      }
    } catch (_: Exception) {}
    val o = JSObject()
    o.put("handle", handle)
    if (ipv4 != null) o.put("ipv4", ipv4)
    invoke.resolve(o)
  }

  /** Paired devices, plus whatever a short scan finds. */
  @SuppressLint("MissingPermission")
  @Command
  fun devices(invoke: Invoke) {
    val args = invoke.parseArgs(MeshDevicesArgs::class.java)
    later(invoke) {
      val a = adapter() ?: throw IOException("no Bluetooth")
      val bonded = (a.bondedDevices ?: emptySet()).map { it.address }.toSet()
      val found = ConcurrentHashMap.newKeySet<String>()
      if (args.scanSecs > 0) synchronized(discoveryLock) {
        val finished = CountDownLatch(1)
        val rx = object : BroadcastReceiver() {
          override fun onReceive(c: Context, i: Intent) {
            when (i.action) {
              BluetoothAdapter.ACTION_DISCOVERY_FINISHED -> finished.countDown()
              BluetoothDevice.ACTION_FOUND -> {
                val device: BluetoothDevice? = if (Build.VERSION.SDK_INT >= 33)
                  i.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE, BluetoothDevice::class.java)
                else @Suppress("DEPRECATION") i.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE)
                device?.let { found.add(it.address) }
              }
            }
          }
        }
        val filter = IntentFilter(BluetoothDevice.ACTION_FOUND).apply {
          addAction(BluetoothAdapter.ACTION_DISCOVERY_FINISHED)
        }
        if (Build.VERSION.SDK_INT >= 33) activity.registerReceiver(rx, filter, Context.RECEIVER_EXPORTED)
        else @Suppress("DEPRECATION") activity.registerReceiver(rx, filter)
        try {
          if (a.isDiscovering || a.startDiscovery()) {
            finished.await(args.scanSecs.coerceIn(15, 25).toLong(), TimeUnit.SECONDS)
          } else if (bonded.isEmpty()) {
            throw IOException("Android could not start Bluetooth discovery")
          }
        } finally {
          try { a.cancelDiscovery() } catch (_: Exception) {}
          activity.unregisterReceiver(rx)
        }
      }
      val list = JSArray()
      for (addr in bonded + found) {
        val d = JSObject()
        d.put("address", addr)
        d.put("bonded", addr in bonded)
        d.put("discovered", addr in found)
        list.put(d)
      }
      val o = JSObject()
      o.put("devices", list)
      o
    }
  }
}
