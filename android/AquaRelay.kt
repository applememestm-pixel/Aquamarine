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
import android.content.BroadcastReceiver
import android.app.ActivityManager
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
import android.provider.OpenableColumns
import android.content.ContentValues
import android.webkit.MimeTypeMap
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
class CallArgs {
  var on: Boolean = false
  /** The notification's line, in the UI language. */
  var text: String? = null
}

@InvokeArg
class SyncArgs {
  /** The reminder after a reboot, in the UI language. */
  var reminder: String? = null
}

@InvokeArg
class UrlArgs {
  var url: String = ""
}

@InvokeArg
class PipArgs {
  var w: Int = 16
  var h: Int = 9
}

@InvokeArg
class SecureArgs {
  var on: Boolean = false
}

@InvokeArg
class WidgetArgs {
  var data: String = "{}"
}

/** ksWrap: the vault master key (hex) to wrap. */
@InvokeArg
class PushArgs {
  /** Whose: the account slot on this phone (each has its own token and
   *  endpoint — two accounts' relays can't tell they are one phone). */
  var instance: String = "main"
  var on: Boolean = false
  /** The notification text, in the UI language ("New message"). */
  var text: String? = null
}

@InvokeArg
class KsWrapArgs {
  var key: String = ""
}

/** ksUnwrap: a blob made by ksWrap (base64). */
@InvokeArg
class KsUnwrapArgs {
  var blob: String = ""
}

/** ksNote: a stage of the key's move, for the system log. */
@InvokeArg
class KsNoteArgs {
  var stage: String = ""
}

/** AQUA-SEC-013: the vault master key is kept wrapped by an AES-256-GCM key
 *  that lives in the Android Keystore and never leaves it. A copy of the
 *  app's folder without this device's Keystore can't open the vault. */
object AquaKeystore {
  private const val ALIAS = "aqua-vault-wrap"

  private fun existing(): javax.crypto.SecretKey? {
    val ks = java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    return ks.getKey(ALIAS, null) as? javax.crypto.SecretKey
  }

  /** For wrapping only: opening a blob must never make a new key. */
  private fun existingOrNew(): javax.crypto.SecretKey {
    existing()?.let { return it }
    val gen = javax.crypto.KeyGenerator.getInstance(android.security.keystore.KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    gen.init(
      android.security.keystore.KeyGenParameterSpec.Builder(
        ALIAS,
        android.security.keystore.KeyProperties.PURPOSE_ENCRYPT or android.security.keystore.KeyProperties.PURPOSE_DECRYPT
      )
        .setBlockModes(android.security.keystore.KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(android.security.keystore.KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build()
    )
    return gen.generateKey()
  }

  /** Some Keystores fail for a moment (right after a start of the phone):
   *  three tries. A blob that doesn't match the key fails at once. */
  private fun <T> retried(op: () -> T): T {
    var tries = 0
    while (true) {
      try {
        return op()
      } catch (e: javax.crypto.AEADBadTagException) {
        throw IllegalStateException("ks:badtag")
      } catch (e: Exception) {
        if (++tries >= 3) throw e
        Thread.sleep(150L * tries)
      }
    }
  }

  /** iv (12) || ciphertext+tag, base64. */
  fun wrap(plain: ByteArray): String = retried {
    val c = javax.crypto.Cipher.getInstance("AES/GCM/NoPadding")
    c.init(javax.crypto.Cipher.ENCRYPT_MODE, existingOrNew())
    val out = c.iv + c.doFinal(plain)
    Base64.encodeToString(out, Base64.NO_WRAP)
  }

  fun unwrap(blob: String): ByteArray {
    val raw = Base64.decode(blob, Base64.NO_WRAP)
    require(raw.size > 12 + 16) { "short blob" }
    return retried {
      val key = existing() ?: throw IllegalStateException("ks:nokey")
      val c = javax.crypto.Cipher.getInstance("AES/GCM/NoPadding")
      c.init(javax.crypto.Cipher.DECRYPT_MODE, key, javax.crypto.spec.GCMParameterSpec(128, raw.copyOfRange(0, 12)))
      c.doFinal(raw.copyOfRange(12, raw.size))
    }
  }
}

// The annotation is what keeps this class and its commands in the shrunk
// release APK (tools/check_android.py).
@TauriPlugin
class AquaRelayPlugin(private val activity: Activity) : Plugin(activity) {
  /** Wrap the vault master key with the Keystore key (state.rs). */
  @Command
  fun ksWrap(invoke: Invoke) {
    try {
      val a = invoke.parseArgs(KsWrapArgs::class.java)
      val bytes = a.key.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
      invoke.resolve(JSObject().put("blob", AquaKeystore.wrap(bytes)))
    } catch (e: Exception) {
      invoke.reject("keystore: " + e.message)
    }
  }

  /** The vault master key (hex) from a ksWrap blob. */
  @Command
  fun ksUnwrap(invoke: Invoke) {
    try {
      val a = invoke.parseArgs(KsUnwrapArgs::class.java)
      val key = AquaKeystore.unwrap(a.blob).joinToString("") { "%02x".format(it) }
      invoke.resolve(JSObject().put("key", key))
    } catch (e: Exception) {
      invoke.reject("keystore: " + e.message)
    }
  }

  /** How many times this phone has been started (-1: it doesn't say). */
  @Command
  fun ksBootCount(invoke: Invoke) {
    val n = Settings.Global.getInt(activity.contentResolver, Settings.Global.BOOT_COUNT, -1)
    invoke.resolve(JSObject().put("boot", n))
  }

  /** Where the key's move into the Keystore stands (no key material). */
  @Command
  fun ksNote(invoke: Invoke) {
    val a = invoke.parseArgs(KsNoteArgs::class.java)
    android.util.Log.i("AquaKeystore", a.stage)
    invoke.resolve(JSObject().put("ok", true))
  }

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

  /** The messenger stays connected in the background (AquaSyncService):
   *  without it Android froze the app and messages came only when opened. */
  @Command
  fun syncStart(invoke: Invoke) {
    AquaPrefs.setSync(activity, true)
    invoke.parseArgs(SyncArgs::class.java).reminder?.let { AquaPrefs.setReminder(activity, it) }
    // The app is open again: the reminder after a reboot has done its job.
    (activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(7793)
    if (Build.VERSION.SDK_INT >= 33 &&
      activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
      activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7791)
    }
    val i = Intent(activity, AquaSyncService::class.java)
    try {
      if (Build.VERSION.SDK_INT >= 26) activity.startForegroundService(i) else activity.startService(i)
      invoke.resolve(JSObject().put("ok", true))
    } catch (e: Exception) {
      invoke.reject(e.message ?: "can't start")
    }
  }

  /** A call is on (AquaCallService) or over. Started only while the app is
   *  on screen: Android refuses microphone / camera services from the
   *  background. The page repeats the call until it succeeds. */
  @Command
  fun callService(invoke: Invoke) {
    val a = invoke.parseArgs(CallArgs::class.java)
    val i = Intent(activity, AquaCallService::class.java)
    try {
      if (!a.on) {
        activity.stopService(i)
        AquaCallService.types = -1
        invoke.resolve(JSObject().put("ok", true))
        return
      }
      if (activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
        invoke.reject("no microphone permission yet")
        return
      }
      if (AquaCallService.types == AquaCallService.wantedTypes(activity)) {
        invoke.resolve(JSObject().put("ok", true))
        return
      }
      val state = ActivityManager.RunningAppProcessInfo()
      ActivityManager.getMyMemoryState(state)
      if (state.importance > ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND) {
        invoke.reject("the app is not on screen")
        return
      }
      a.text?.let { i.putExtra("text", it) }
      // startService, not startForegroundService: if Android refuses the
      // foreground type, the service just stops instead of crashing the app.
      activity.startService(i)
      invoke.resolve(JSObject().put("ok", true))
    } catch (e: Exception) {
      invoke.reject(e.message ?: "can't start")
    }
  }

  /** Waking through UnifiedPush (AquaPush): register with the distributor
   *  installed on this phone (ntfy, NextPush… — no Google), or stop. */
  @Command
  fun pushSet(invoke: Invoke) {
    val a = invoke.parseArgs(PushArgs::class.java)
    a.text?.let { AquaPush.setText(activity, it) }
    if (a.on) {
      if (Build.VERSION.SDK_INT >= 33 &&
        activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
        activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7794)
      }
      val d = AquaPush.register(activity, a.instance)
      invoke.resolve(JSObject().put("distributor", d ?: "").put("ok", d != null))
    } else {
      AquaPush.unregister(activity, a.instance)
      invoke.resolve(JSObject().put("ok", true))
    }
  }

  /** Where this phone is woken (the endpoint the distributor gave), and
   *  which distributors are installed. */
  @Command
  fun pushState(invoke: Invoke) {
    val a = invoke.parseArgs(PushArgs::class.java)
    val list = JSArray()
    AquaPush.distributors(activity).forEach { list.put(it) }
    invoke.resolve(JSObject()
      .put("on", AquaPush.on(activity, a.instance))
      .put("endpoint", AquaPush.endpoint(activity, a.instance) ?: "")
      .put("distributor", AquaPush.distributor(activity) ?: "")
      .put("distributors", list))
  }

  @Command
  fun syncStop(invoke: Invoke) {
    AquaPrefs.setSync(activity, false)
    activity.stopService(Intent(activity, AquaSyncService::class.java))
    invoke.resolve(JSObject().put("ok", true))
  }

  /** Battery optimisation off for the app (the system's own dialog): a
   *  phone in Doze otherwise cuts the connection anyway. */
  @Command
  fun askBattery(invoke: Invoke) {
    val pm = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
    if (Build.VERSION.SDK_INT >= 23 && !pm.isIgnoringBatteryOptimizations(activity.packageName)) {
      try {
        @Suppress("BatteryLife")
        val i = Intent(android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
          .setData(android.net.Uri.parse("package:" + activity.packageName))
        activity.startActivity(i)
      } catch (_: Exception) { }
      invoke.resolve(JSObject().put("ok", false))
    } else {
      invoke.resolve(JSObject().put("ok", true))
    }
  }

  /** Picture-in-picture: the app shrinks into a floating window while a
   *  video plays (the page made the video fill the window first). */
  @Command
  fun enterPip(invoke: Invoke) {
    val a = invoke.parseArgs(PipArgs::class.java)
    if (Build.VERSION.SDK_INT < 26) { invoke.reject("picture-in-picture needs Android 8"); return }
    // Android allows ratios between 1:2.39 and 2.39:1.
    val w = a.w.coerceAtLeast(1); val h = a.h.coerceAtLeast(1)
    val r = (w.toDouble() / h).coerceIn(1 / 2.39, 2.39)
    val ratio = if (r >= 1) android.util.Rational((r * 1000).toInt(), 1000) else android.util.Rational(1000, (1000 / r).toInt())
    val ok = activity.enterPictureInPictureMode(android.app.PictureInPictureParams.Builder().setAspectRatio(ratio).build())
    if (ok) invoke.resolve(JSObject().put("ok", true)) else invoke.reject("picture-in-picture was refused")
  }

  /** No screenshots or screen recording while a view-once photo is open
   *  or in a chat with sharing off (FLAG_SECURE). */
  @Command
  fun secure(invoke: Invoke) {
    val a = invoke.parseArgs(SecureArgs::class.java)
    activity.runOnUiThread {
      val f = android.view.WindowManager.LayoutParams.FLAG_SECURE
      if (a.on) activity.window.addFlags(f) else activity.window.clearFlags(f)
    }
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

  /** What the home-screen widget shows (AquaWidget.kt): unread chats. */
  @Command
  fun updateWidget(invoke: Invoke) {
    val args = invoke.parseArgs(WidgetArgs::class.java)
    try {
      AquaWidget.store(activity.applicationContext, args.data)
      invoke.resolve(JSObject().put("ok", true))
    } catch (e: Exception) {
      invoke.reject("widget: ${e.message}")
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
      .setSmallIcon(R.drawable.ic_stat_aquamarine)
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


/** The messenger's own foreground service: a quiet persistent notification
 *  that keeps the process (and its connection to the relay) alive while the
 *  app is in the background, so new messages are received and announced. */
class AquaSyncService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= 26) {
      val ch = NotificationChannel("sync", "Connection", NotificationManager.IMPORTANCE_MIN)
      ch.setShowBadge(false)
      nm.createNotificationChannel(ch)
    }
    val open = PendingIntent.getActivity(
      this, 1, packageManager.getLaunchIntentForPackage(packageName),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "sync") else Notification.Builder(this)
    val n = b.setContentTitle("Aquamarine")
      .setContentText("Connected — new messages arrive in the background")
      .setSmallIcon(R.drawable.ic_stat_aquamarine)
      .setOngoing(false)
      .setGroup("aquamarine.connection")
      .setOnlyAlertOnce(true)
      .setContentIntent(open)
      .build()
    try {
      if (Build.VERSION.SDK_INT >= 34) {
        startForeground(7791, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_REMOTE_MESSAGING)
      } else {
        startForeground(7791, n)
      }
    } catch (_: Exception) {
      stopSelf()
      return START_NOT_STICKY
    }
    return START_STICKY
  }
}

/** What native code must know without the app running. */
object AquaPrefs {
  private const val FILE = "aqua_native"
  fun setSync(c: Context, on: Boolean) =
    c.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit().putBoolean("sync", on).apply()
  fun sync(c: Context): Boolean =
    c.getSharedPreferences(FILE, Context.MODE_PRIVATE).getBoolean("sync", false)
  fun setReminder(c: Context, text: String) =
    c.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit().putString("reminder", text).apply()
  fun reminder(c: Context): String =
    c.getSharedPreferences(FILE, Context.MODE_PRIVATE).getString("reminder", null)
      ?: "Open the app to receive new messages again"
}

/** During a call: an ongoing notification that lets the microphone and the
 *  camera keep working when the app is in the background or the screen is
 *  off. Without it Android cuts them and the call goes silent. */
class AquaCallService : Service() {
  companion object {
    /** The foreground types it runs with; -1 = not running. */
    @Volatile var types: Int = -1

    fun wantedTypes(c: Context): Int {
      if (Build.VERSION.SDK_INT < 30) return 0
      var t = ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
      if (c.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
        t = t or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
      }
      return t
    }
  }

  private var wake: PowerManager.WakeLock? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= 26) {
      nm.createNotificationChannel(NotificationChannel("call", "Calls", NotificationManager.IMPORTANCE_LOW))
    }
    val open = PendingIntent.getActivity(
      this, 2, packageManager.getLaunchIntentForPackage(packageName),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "call") else Notification.Builder(this)
    val n = b.setContentTitle("Aquamarine")
      .setContentText(intent?.getStringExtra("text") ?: "Call in progress — tap to return")
      .setSmallIcon(R.drawable.ic_stat_aquamarine)
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setContentIntent(open)
      .build()
    val want = wantedTypes(this)
    try {
      if (Build.VERSION.SDK_INT >= 30) startForeground(7792, n, want) else startForeground(7792, n)
      types = want
    } catch (_: Exception) {
      // Already in the foreground: keep going with the types it has.
      if (types == -1) {
        stopSelf()
        return START_NOT_STICKY
      }
    }
    if (wake == null) {
      wake = (getSystemService(Context.POWER_SERVICE) as PowerManager)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "aquamarine:call").also { it.acquire(12 * 60 * 60 * 1000L) }
    }
    return START_NOT_STICKY
  }

  override fun onTaskRemoved(rootIntent: Intent?) {
    stopSelf()
  }

  override fun onDestroy() {
    types = -1
    wake?.let { if (it.isHeld) it.release() }
    wake = null
    super.onDestroy()
  }
}

/** After a reboot or an update Android does not restart the messenger's
 *  connection, and the app can't connect without being opened: say so once,
 *  if background messages are on. */
class AquaBootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent?) {
    val action = intent?.action ?: return
    if (action != Intent.ACTION_BOOT_COMPLETED && action != Intent.ACTION_MY_PACKAGE_REPLACED) return
    if (!AquaPrefs.sync(context)) return
    try {
      val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
      if (Build.VERSION.SDK_INT >= 26) {
        nm.createNotificationChannel(NotificationChannel("resume", "Reconnect reminder", NotificationManager.IMPORTANCE_DEFAULT))
      }
      val open = PendingIntent.getActivity(
        context, 3, context.packageManager.getLaunchIntentForPackage(context.packageName),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
      )
      val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(context, "resume") else Notification.Builder(context)
      nm.notify(7793, b.setContentTitle("Aquamarine")
        .setContentText(AquaPrefs.reminder(context))
        .setSmallIcon(R.drawable.ic_stat_aquamarine)
        .setAutoCancel(true)
        .setContentIntent(open)
        .build())
    } catch (_: Exception) {
      // No notification permission: nothing to say.
    }
  }
}

@InvokeArg
class GalleryArgs {
  var limit: Int = 60
  /** An album's bucket id, "*video" for all videos, "" for everything. */
  var bucket: String = ""
}

@InvokeArg
class GalleryFileArgs {
  var uri: String = ""
  var source: String = ""
  var name: String = ""
}

private class GalleryAlbum(val id: String, val name: String, var count: Int, val coverId: Long, val coverVideo: Boolean)

@TauriPlugin
class AquaGalleryPlugin(private val activity: Activity) : Plugin(activity) {
  private val pool = Executors.newSingleThreadExecutor()

  @Command
  fun fileInfo(invoke: Invoke) {
    val args = invoke.parseArgs(GalleryFileArgs::class.java)
    val uri = Uri.parse(args.uri)
    if (uri.scheme != "content") { invoke.reject("Expected a content URI"); return }
    try {
      var name = ""
      activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) name = cursor.getString(0) ?: ""
      }
      invoke.resolve(JSObject().put("name", name))
    } catch (e: Exception) { invoke.reject(e.message ?: "Could not read file information") }
  }

  @Command
  fun saveDownload(invoke: Invoke) {
    val args = invoke.parseArgs(GalleryFileArgs::class.java)
    if (Build.VERSION.SDK_INT < 29) {
      invoke.resolve(JSObject().put("saved", false))
      return
    }
    pool.execute {
      var uri: Uri? = null
      try {
        val source = File(args.source).canonicalFile
        val own = activity.filesDir.parentFile!!.canonicalFile
        if (!source.path.startsWith(own.path + File.separator) || !source.isFile) throw IllegalArgumentException("Invalid download source")
        val name = File(args.name).name.ifBlank { source.name }
        val resolver = activity.contentResolver
        val values = ContentValues().apply {
          put(MediaStore.Downloads.DISPLAY_NAME, name)
          put(MediaStore.Downloads.MIME_TYPE, MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase()) ?: "application/octet-stream")
          put(MediaStore.Downloads.RELATIVE_PATH, "Download/Aquamarine")
          put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val target = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: throw IllegalStateException("Could not create download")
        uri = target
        resolver.openOutputStream(target, "w")?.use { output -> source.inputStream().use { it.copyTo(output) } }
          ?: throw IllegalStateException("Could not open download")
        if (resolver.update(target, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null) == 0) {
          throw IllegalStateException("Could not publish download")
        }
        invoke.resolve(JSObject().put("saved", true))
      } catch (e: Exception) {
        uri?.let { runCatching { activity.contentResolver.delete(it, null, null) } }
        invoke.reject(e.message ?: "Could not save download")
      }
    }
  }

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

/** UnifiedPush (https://unifiedpush.org, Android spec AND_3), without a
 *  library: the distributor app the person chose (ntfy, NextPush…) holds the
 *  one connection; the relay posts a fixed "wake up" to the endpoint it gives
 *  (aqua-server push.rs). Nothing of a message is in it. Every intent from a
 *  distributor carries the random token we registered with — one without it
 *  (another app) is ignored, so no one else can set where we are woken. */
object AquaPush {
  private const val FILE = "aqua_push"
  const val REGISTER = "org.unifiedpush.android.distributor.REGISTER"
  const val UNREGISTER = "org.unifiedpush.android.distributor.UNREGISTER"
  const val ACK = "org.unifiedpush.android.distributor.MESSAGE_ACK"

  private fun prefs(c: Context) = c.getSharedPreferences(FILE, Context.MODE_PRIVATE)
  private fun safe(i: String) = i.filter { it.isLetterOrDigit() || it == '-' || it == '_' }.take(32).ifEmpty { "main" }
  fun on(c: Context, i: String) = prefs(c).getBoolean("on:" + safe(i), false)
  fun endpoint(c: Context, i: String): String? = prefs(c).getString("endpoint:" + safe(i), null)
  fun distributor(c: Context): String? = prefs(c).getString("distributor", null)
  fun token(c: Context, i: String): String? = prefs(c).getString("token:" + safe(i), null)
  fun setText(c: Context, t: String) = prefs(c).edit().putString("text", t).apply()
  fun text(c: Context): String = prefs(c).getString("text", null) ?: "New message"
  fun setEndpoint(c: Context, i: String, e: String?) = prefs(c).edit().putString("endpoint:" + safe(i), e).apply()
  /** The account slot a distributor's token belongs to (null: not ours). */
  fun instanceOf(c: Context, token: String?): String? {
    if (token.isNullOrEmpty()) return null
    return prefs(c).all.entries.firstOrNull { it.key.startsWith("token:") && it.value == token }?.key?.removePrefix("token:")
  }

  /** Installed distributors (package names). */
  fun distributors(c: Context): List<String> {
    val pm = c.packageManager
    val found = if (Build.VERSION.SDK_INT >= 33) {
      pm.queryBroadcastReceivers(Intent(REGISTER), PackageManager.ResolveInfoFlags.of(PackageManager.GET_RESOLVED_FILTER.toLong()))
    } else {
      @Suppress("DEPRECATION") pm.queryBroadcastReceivers(Intent(REGISTER), PackageManager.GET_RESOLVED_FILTER)
    }
    return found.mapNotNull { it.activityInfo?.packageName }.filter { it != c.packageName }.distinct()
  }

  /** Identifies this app to the distributor (AND_3: it reads the creator). */
  private fun pi(c: Context): PendingIntent =
    PendingIntent.getBroadcast(c, 0, Intent("org.unifiedpush.android.connector.AUTH").setPackage(c.packageName), PendingIntent.FLAG_IMMUTABLE)

  /** Register with the chosen (or the only / first) distributor; null when
   *  none is installed. The endpoint comes later (AquaPushReceiver). */
  fun register(c: Context, instance: String): String? {
    val i = safe(instance)
    val all = distributors(c)
    val d = distributor(c)?.takeIf { it in all } ?: all.firstOrNull() ?: return null
    val token = token(c, i) ?: java.util.UUID.randomUUID().toString().also { prefs(c).edit().putString("token:$i", it).apply() }
    prefs(c).edit().putBoolean("on:$i", true).putString("distributor", d).apply()
    c.sendBroadcast(Intent(REGISTER).setPackage(d)
      .putExtra("token", token)
      .putExtra("application", c.packageName)
      .putExtra("message", "Aquamarine: new messages")
      .putExtra("pi", pi(c)))
    return d
  }

  fun unregister(c: Context, instance: String) {
    val i = safe(instance)
    val d = distributor(c)
    val token = token(c, i)
    if (d != null && token != null) {
      c.sendBroadcast(Intent(UNREGISTER).setPackage(d).putExtra("token", token).putExtra("application", c.packageName).putExtra("pi", pi(c)))
    }
    // A new token next time: the old endpoint stays useless to whoever kept it.
    prefs(c).edit().putBoolean("on:$i", false).remove("endpoint:$i").remove("token:$i").apply()
  }

  /** The relay's wake signal: say so (no sender, no text — there is none),
   *  unless the app is on screen; keep the connection going if that's on. */
  fun woken(c: Context) {
    if (AquaPrefs.sync(c)) {
      try {
        val i = Intent(c, AquaSyncService::class.java)
        if (Build.VERSION.SDK_INT >= 26) c.startForegroundService(i) else c.startService(i)
      } catch (_: Exception) { /* not allowed from the background here: the notification still says it */ }
    }
    val state = ActivityManager.RunningAppProcessInfo()
    ActivityManager.getMyMemoryState(state)
    if (state.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND) return
    try {
      val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
      if (Build.VERSION.SDK_INT >= 26) {
        nm.createNotificationChannel(NotificationChannel("push", "New messages", NotificationManager.IMPORTANCE_HIGH))
      }
      val open = PendingIntent.getActivity(
        c, 4, c.packageManager.getLaunchIntentForPackage(c.packageName),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
      )
      val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(c, "push") else Notification.Builder(c)
      // One notification for all of them: the app shows the chats when opened.
      nm.notify(7794, b.setContentTitle("Aquamarine")
        .setContentText(text(c))
        .setSmallIcon(R.drawable.ic_stat_aquamarine)
        .setAutoCancel(true)
        .setContentIntent(open)
        .build())
    } catch (_: Exception) {
      // No notification permission: nothing to say.
    }
  }
}

/** Intents from the UnifiedPush distributor. Exported (another app sends
 *  them); each must carry our token. */
class AquaPushReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent?) {
    val i = intent ?: return
    val mine = i.getStringExtra("token")
    val inst = AquaPush.instanceOf(context, mine) ?: return
    if (!AquaPush.on(context, inst)) return
    when (i.action) {
      "org.unifiedpush.android.connector.NEW_ENDPOINT" -> {
        val e = i.getStringExtra("endpoint") ?: return
        // https only (the relay refuses anything else too). The app sends
        // it to the relay when it next looks (push.rs, every minute while
        // connected, and on each connection).
        if (e.startsWith("https://") && e.length <= 2048) AquaPush.setEndpoint(context, inst, e)
      }
      "org.unifiedpush.android.connector.MESSAGE" -> {
        i.getStringExtra("id")?.let { id ->
          AquaPush.distributor(context)?.let { d -> context.sendBroadcast(Intent(AquaPush.ACK).setPackage(d).putExtra("token", mine).putExtra("id", id)) }
        }
        AquaPush.woken(context)
      }
      "org.unifiedpush.android.connector.UNREGISTERED", "org.unifiedpush.android.connector.REGISTRATION_FAILED" ->
        AquaPush.setEndpoint(context, inst, null)
    }
  }
}
