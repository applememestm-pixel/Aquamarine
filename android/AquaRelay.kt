package app.aquamarine.messenger

// Android halves of two features (copied next to MainActivity by CI and
// android/patch-project.sh):
//  * AquaGalleryPlugin — the recent photos and videos for the attach sheet
//    and its album menu (crates/aqua-client-ui/src/gallery.rs), like Telegram's.
//  * AquaRelayPlugin + AquaRelayService — "Server on this device" (and the
//    relaunch after switching accounts, crates/aqua-client-ui/src/accounts.rs).
//  * AquaUpdatePlugin — hands a downloaded update to the system installer
//    (crates/aqua-client-ui/src/updates.rs).
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
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File
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

@InvokeArg
class UrlArgs {
  var url: String = ""
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

  /** A web or mail link in the browser / mail app ("What's new", About,
   *  links in messages): Rust can't spawn xdg-open here. */
  @Command
  fun openUrl(invoke: Invoke) {
    val args = invoke.parseArgs(UrlArgs::class.java)
    try {
      val i = Intent(Intent.ACTION_VIEW, Uri.parse(args.url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.runOnUiThread { activity.startActivity(i) }
      invoke.resolve(JSObject().put("ok", true))
    } catch (e: Exception) {
      invoke.reject("no app can open this link")
    }
  }

  /** Start the app over in a fresh process: another account was picked and
   *  the data directory is chosen once, at start (accounts.rs). */
  @Command
  fun relaunch(invoke: Invoke) {
    invoke.resolve(JSObject().put("ok", true))
    val launch = activity.packageManager.getLaunchIntentForPackage(activity.packageName) ?: return
    val restart = Intent.makeRestartActivityTask(launch.component)
    activity.runOnUiThread {
      activity.startActivity(restart)
      Runtime.getRuntime().exit(0)
    }
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
  /** An album's bucket id, "*video" for all videos, "" for everything. */
  var bucket: String = ""
}

private class GalleryAlbum(val id: String, val name: String, var count: Int, val coverId: Long, val coverVideo: Boolean)

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
        var sel = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (${MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE},${MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO})"
        var selArgs: Array<String>? = null
        when {
          args.bucket == "*video" -> sel = "${MediaStore.Files.FileColumns.MEDIA_TYPE} = ${MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO}"
          args.bucket.isNotEmpty() -> { sel += " AND bucket_id = ?"; selArgs = arrayOf(args.bucket) }
        }
        activity.contentResolver.query(uri, proj, sel, selArgs, "${MediaStore.Files.FileColumns.DATE_ADDED} DESC")?.use { c ->
          var n = 0
          while (c.moveToNext() && n < args.limit.coerceIn(1, 200)) {
            val id = c.getLong(0)
            val video = c.getInt(1) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
            val base = if (video) MediaStore.Video.Media.EXTERNAL_CONTENT_URI else MediaStore.Images.Media.EXTERNAL_CONTENT_URI
            val item = ContentUris.withAppendedId(base, id)
            val thumb = thumbOf(id, video, 256)
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

  /** A small square-ish JPEG preview as a data: URL ("" if there's none). */
  private fun thumbOf(id: Long, video: Boolean, px: Int): String = try {
    val base = if (video) MediaStore.Video.Media.EXTERNAL_CONTENT_URI else MediaStore.Images.Media.EXTERNAL_CONTENT_URI
    val item = ContentUris.withAppendedId(base, id)
    val bmp: Bitmap = if (Build.VERSION.SDK_INT >= 29) activity.contentResolver.loadThumbnail(item, Size(px, px), null)
      else if (video) MediaStore.Video.Thumbnails.getThumbnail(activity.contentResolver, id, MediaStore.Video.Thumbnails.MINI_KIND, null)
      else MediaStore.Images.Thumbnails.getThumbnail(activity.contentResolver, id, MediaStore.Images.Thumbnails.MINI_KIND, null)
    val bos = ByteArrayOutputStream()
    bmp.compress(Bitmap.CompressFormat.JPEG, 72, bos)
    "data:image/jpeg;base64," + Base64.encodeToString(bos.toByteArray(), Base64.NO_WRAP)
  } catch (e: Exception) { "" }

  /** The albums, newest first, like the system picker's menu: "All media",
   *  "All videos", then every folder with a count and its newest picture. */
  @Command
  fun albums(invoke: Invoke) {
    if (!granted()) { invoke.resolve(JSObject().put("granted", false).put("albums", JSArray())); return }
    pool.execute {
      try {
        val uri = MediaStore.Files.getContentUri("external")
        val proj = arrayOf(MediaStore.Files.FileColumns._ID, MediaStore.Files.FileColumns.MEDIA_TYPE, "bucket_id", "bucket_display_name")
        val sel = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (${MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE},${MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO})"
        val byId = LinkedHashMap<String, GalleryAlbum>()
        var total = 0; var videos = 0
        var first: Pair<Long, Boolean>? = null
        var firstVideo: Long? = null
        activity.contentResolver.query(uri, proj, sel, null, "${MediaStore.Files.FileColumns.DATE_ADDED} DESC")?.use { c ->
          while (c.moveToNext()) {
            val id = c.getLong(0)
            val video = c.getInt(1) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
            val bid = c.getString(2) ?: continue
            total++
            if (first == null) first = Pair(id, video)
            if (video) { videos++; if (firstVideo == null) firstVideo = id }
            val a = byId[bid]
            if (a == null) byId[bid] = GalleryAlbum(bid, c.getString(3) ?: "", 1, id, video) else a.count++
          }
        }
        val out = JSArray()
        out.put(JSObject().put("id", "").put("name", "").put("count", total).put("cover", first?.let { thumbOf(it.first, it.second, 128) } ?: ""))
        if (videos > 0) out.put(JSObject().put("id", "*video").put("name", "").put("count", videos).put("cover", firstVideo?.let { thumbOf(it, true, 128) } ?: ""))
        for (a in byId.values) out.put(JSObject().put("id", a.id).put("name", a.name).put("count", a.count).put("cover", thumbOf(a.coverId, a.coverVideo, 128)))
        invoke.resolve(JSObject().put("granted", true).put("albums", out))
      } catch (e: Exception) {
        invoke.reject(e.message ?: "gallery")
      }
    }
  }
}

@InvokeArg
class InstallArgs {
  var path: String = ""
}

/** Updates: the APK updates.rs downloaded and checked goes to the system
 *  installer through the app's FileProvider (cache-path). Android asks once
 *  to allow installs from Aquamarine; until then we open that setting. */
@TauriPlugin
class AquaUpdatePlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun install(invoke: Invoke) {
    val args = invoke.parseArgs(InstallArgs::class.java)
    if (Build.VERSION.SDK_INT >= 26 && !activity.packageManager.canRequestPackageInstalls()) {
      activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + activity.packageName)))
      invoke.resolve(JSObject().put("needsPermission", true))
      return
    }
    // A build signed with another key can't replace this one: Android only
    // says "App not installed". Find out first and say what to do.
    try {
      val mine = signersOf(activity.packageManager.let { pm ->
        if (Build.VERSION.SDK_INT >= 28) pm.getPackageInfo(activity.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        else @Suppress("DEPRECATION") pm.getPackageInfo(activity.packageName, PackageManager.GET_SIGNATURES)
      })
      val theirs = signersOf(activity.packageManager.let { pm ->
        if (Build.VERSION.SDK_INT >= 28) pm.getPackageArchiveInfo(args.path, PackageManager.GET_SIGNING_CERTIFICATES)
        else @Suppress("DEPRECATION") pm.getPackageArchiveInfo(args.path, PackageManager.GET_SIGNATURES)
      })
      if (mine.isNotEmpty() && theirs.isNotEmpty() && mine.intersect(theirs).isEmpty()) {
        invoke.resolve(JSObject().put("signatureMismatch", true))
        return
      }
    } catch (e: Exception) { /* can't tell: let the installer decide */ }
    try {
      var f = File(args.path)
      // The FileProvider only serves the cache directory.
      if (!f.canonicalPath.startsWith(activity.cacheDir.canonicalPath + File.separator)) {
        val dir = File(activity.cacheDir, "updates").apply { mkdirs() }
        val copy = File(dir, f.name)
        f.copyTo(copy, overwrite = true)
        f = copy
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

  /** SHA-256 of each signing certificate of a package (installed or a file). */
  private fun signersOf(info: android.content.pm.PackageInfo?): Set<String> {
    if (info == null) return emptySet()
    val sigs = if (Build.VERSION.SDK_INT >= 28) {
      val si = info.signingInfo ?: return emptySet()
      if (si.hasMultipleSigners()) si.apkContentsSigners else si.signingCertificateHistory
    } else @Suppress("DEPRECATION") info.signatures
    val md = java.security.MessageDigest.getInstance("SHA-256")
    return (sigs ?: emptyArray()).map { md.digest(it.toByteArray()).joinToString("") { b -> "%02x".format(b) } }.toSet()
  }
}
