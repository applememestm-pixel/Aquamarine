package app.aquamarine.messenger

// Android halves of two features (copied next to MainActivity by CI and
// android/patch-project.sh):
//  * AquaGalleryPlugin — the recent photos and videos for the attach sheet
//    (crates/aqua-client-ui/src/gallery.rs), like Telegram's.
//  * AquaRelayPlugin + AquaRelayService — "Server on this device".
//
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
import android.content.ContentUris
import android.graphics.Bitmap
import android.provider.MediaStore
import android.util.Base64
import android.util.Size
import app.tauri.plugin.JSArray
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
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


@InvokeArg
class GalleryArgs {
  var limit: Int = 60
}

@TauriPlugin
class AquaGalleryPlugin(private val activity: Activity) : Plugin(activity) {
  private val pool = Executors.newSingleThreadExecutor()

  private fun needed(): Array<String> =
    if (Build.VERSION.SDK_INT >= 33) arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO)
    else arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)

  private fun granted(): Boolean {
    if (Build.VERSION.SDK_INT >= 34 &&
      activity.checkSelfPermission("android.permission.READ_MEDIA_VISUAL_USER_SELECTED") == PackageManager.PERMISSION_GRANTED) return true
    return needed().all { activity.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }
  }

  @Command
  fun ask(invoke: Invoke) {
    if (!granted()) activity.requestPermissions(needed(), 7791)
    invoke.resolve(JSObject().put("granted", granted()))
  }

  /** The newest photos and videos: content URIs with small JPEG thumbnails. */
  @Command
  fun recent(invoke: Invoke) {
    val args = invoke.parseArgs(GalleryArgs::class.java)
    if (!granted()) { invoke.resolve(JSObject().put("granted", false).put("items", JSArray())); return }
    pool.execute {
      val out = JSArray()
      try {
        val uri = MediaStore.Files.getContentUri("external")
        val proj = arrayOf(MediaStore.Files.FileColumns._ID, MediaStore.Files.FileColumns.MEDIA_TYPE,
          MediaStore.Files.FileColumns.MIME_TYPE, MediaStore.Video.VideoColumns.DURATION)
        val sel = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (${MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE},${MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO})"
        activity.contentResolver.query(uri, proj, sel, null, "${MediaStore.Files.FileColumns.DATE_ADDED} DESC")?.use { c ->
          var n = 0
          while (c.moveToNext() && n < args.limit.coerceIn(1, 200)) {
            val id = c.getLong(0)
            val video = c.getInt(1) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
            val base = if (video) MediaStore.Video.Media.EXTERNAL_CONTENT_URI else MediaStore.Images.Media.EXTERNAL_CONTENT_URI
            val item = ContentUris.withAppendedId(base, id)
            val thumb = try {
              val bmp: Bitmap = if (Build.VERSION.SDK_INT >= 29) activity.contentResolver.loadThumbnail(item, Size(256, 256), null)
                else if (video) MediaStore.Video.Thumbnails.getThumbnail(activity.contentResolver, id, MediaStore.Video.Thumbnails.MINI_KIND, null)
                else MediaStore.Images.Thumbnails.getThumbnail(activity.contentResolver, id, MediaStore.Images.Thumbnails.MINI_KIND, null)
              val bos = ByteArrayOutputStream()
              bmp.compress(Bitmap.CompressFormat.JPEG, 72, bos)
              "data:image/jpeg;base64," + Base64.encodeToString(bos.toByteArray(), Base64.NO_WRAP)
            } catch (e: Exception) { "" }
            out.put(JSObject().put("uri", item.toString()).put("mime", c.getString(2) ?: "")
              .put("video", video).put("duration", if (video) c.getLong(3) else 0L).put("thumb", thumb))
            n++
          }
        }
        invoke.resolve(JSObject().put("granted", true).put("items", out))
      } catch (e: Exception) {
        invoke.reject(e.message ?: "gallery")
      }
    }
  }
}
