package app.aquamarine.messenger

// "Server on this device" — the Android half. The relay itself runs in Rust
// inside the app process (crates/aqua-client-ui/src/relay_host.rs); this
// foreground service only keeps that process alive with the screen off and
// shows the ongoing notification Android requires. Copied into the generated
// project next to MainActivity by CI and android/patch-project.sh.

import android.Manifest
import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class RelayArgs {
  var port: Int = 7777
}

@TauriPlugin
class AquaRelayPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun start(invoke: Invoke) {
    val args = invoke.parseArgs(RelayArgs::class.java)
    if (Build.VERSION.SDK_INT >= 33 &&
      activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
      activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7790)
    }
    val i = Intent(activity, AquaRelayService::class.java).putExtra("port", args.port)
    if (Build.VERSION.SDK_INT >= 26) activity.startForegroundService(i) else activity.startService(i)
    invoke.resolve(JSObject().put("ok", true))
  }

  @Command
  fun stop(invoke: Invoke) {
    activity.stopService(Intent(activity, AquaRelayService::class.java))
    invoke.resolve(JSObject().put("ok", true))
  }
}

class AquaRelayService : Service() {
  private var wake: PowerManager.WakeLock? = null
  private var wifi: WifiManager.WifiLock? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val port = intent?.getIntExtra("port", 7777) ?: 7777
    val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= 26) {
      nm.createNotificationChannel(NotificationChannel("relay", "Server", NotificationManager.IMPORTANCE_LOW))
    }
    val open = PendingIntent.getActivity(
      this, 0, packageManager.getLaunchIntentForPackage(packageName),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "relay") else Notification.Builder(this)
    val n = b.setContentTitle("Aquamarine server is running")
      .setContentText("Port $port · friends connect to this phone's address")
      .setSmallIcon(applicationInfo.icon)
      .setOngoing(true)
      .setContentIntent(open)
      .build()
    if (Build.VERSION.SDK_INT >= 34) {
      startForeground(7790, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
    } else {
      startForeground(7790, n)
    }
    // Keep the CPU and Wi-Fi awake while serving: a sleeping phone drops its clients.
    if (wake == null) {
      wake = (getSystemService(Context.POWER_SERVICE) as PowerManager)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "aquamarine:relay").also { it.acquire() }
    }
    if (wifi == null) {
      @Suppress("DEPRECATION")
      wifi = (applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager)
        .createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "aquamarine:relay").also { it.acquire() }
    }
    return START_STICKY
  }

  override fun onDestroy() {
    wake?.let { if (it.isHeld) it.release() }
    wifi?.let { if (it.isHeld) it.release() }
    wake = null; wifi = null
    super.onDestroy()
  }
}
