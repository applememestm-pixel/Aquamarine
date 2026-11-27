package app.aquamarine.server

// Aquamarine Server on Android (crates/aqua-server-app): the relay runs in
// Rust inside the app; this foreground service keeps the process alive with
// the screen off and shows the notification Android requires. CI copies it
// next to MainActivity in the generated project.

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
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ServerArgs {
  var port: Int = 7777
}

@InvokeArg
class InstallArgs {
  var path: String = ""
}

@TauriPlugin
class AquaServerPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun start(invoke: Invoke) {
    val args = invoke.parseArgs(ServerArgs::class.java)
    if (Build.VERSION.SDK_INT >= 33 &&
      activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
      activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7790)
    }
    val i = Intent(activity, AquaServerService::class.java).putExtra("port", args.port)
    if (Build.VERSION.SDK_INT >= 26) activity.startForegroundService(i) else activity.startService(i)
    invoke.resolve(JSObject().put("ok", true))
  }

  /** Updates (updates.rs): the APK downloaded here and checked against the
   *  signed sums goes to the system installer through the app's
   *  FileProvider (cache dir) — not a browser download nobody checked.
   *  Android asks once to allow installs from this app. */
  @Command
  fun install(invoke: Invoke) {
    val args = invoke.parseArgs(InstallArgs::class.java)
    if (Build.VERSION.SDK_INT >= 26 && !activity.packageManager.canRequestPackageInstalls()) {
      activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + activity.packageName)))
      invoke.resolve(JSObject().put("needsPermission", true))
      return
    }
    try {
      val f = File(args.path)
      if (!f.canonicalPath.startsWith(activity.cacheDir.canonicalPath + File.separator)) {
        invoke.reject("not a downloaded update")
        return
      }
      val uri = FileProvider.getUriForFile(activity, activity.packageName + ".fileprovider", f)
      val i = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, "application/vnd.android.package-archive")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.startActivity(i)
      invoke.resolve(JSObject().put("started", true))
    } catch (e: Exception) {
      invoke.reject(e.message ?: "install")
    }
  }

  @Command
  fun stop(invoke: Invoke) {
    activity.stopService(Intent(activity, AquaServerService::class.java))
    invoke.resolve(JSObject().put("ok", true))
  }
}

class AquaServerService : Service() {
  private var wake: PowerManager.WakeLock? = null
  private var wifi: WifiManager.WifiLock? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val port = intent?.getIntExtra("port", 7777) ?: 7777
    val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= 26) {
      nm.createNotificationChannel(NotificationChannel("server", "Server", NotificationManager.IMPORTANCE_LOW))
    }
    val open = PendingIntent.getActivity(
      this, 0, packageManager.getLaunchIntentForPackage(packageName),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "server") else Notification.Builder(this)
    val n = b.setContentTitle("Aquamarine Server is running")
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
    if (wake == null) {
      wake = (getSystemService(Context.POWER_SERVICE) as PowerManager)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "aquamarine:server").also { it.acquire() }
    }
    if (wifi == null) {
      @Suppress("DEPRECATION")
      wifi = (applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager)
        .createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "aquamarine:server").also { it.acquire() }
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
