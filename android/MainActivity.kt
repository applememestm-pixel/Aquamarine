package app.aquamarine.messenger

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.SystemClock
import android.util.Base64
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import java.io.ByteArrayOutputStream
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

// Copied over the Tauri-generated file by android/patch-project.ps1 and CI.
class MainActivity : TauriActivity() {
  private var web: WebView? = null
  // The system bars / cutout and the WebView's own size, in device pixels,
  // as JSON for the page (ui/src/lib/insets.ts). The page works out the
  // padding itself from these: env(safe-area-inset-*) came out wrong at an
  // interface scale other than 100 % and with the 3-button navigation bar
  // (the bottom bar and the composer rode high with a gap under them).
  @Volatile private var insetsJson = ""
  private var bars = intArrayOf(0, 0, 0, 0)

  inner class InsetsBridge {
    @JavascriptInterface fun insets(): String = insetsJson
  }

  // Screen sharing in calls (ui/src/lib/androidScreen.ts). The WebView has
  // no getDisplayMedia, so the system's own "start recording / casting?"
  // prompt (MediaProjection, what Gemini Live uses) is asked here; frames of
  // a virtual display come back as JPEG and the page draws them into a
  // canvas whose captureStream() goes out as the call's screen track.
  private var screenAsk: ActivityResultLauncher<Intent>? = null

  inner class ScreenBridge {
    @JavascriptInterface fun start(fps: Int, maxSide: Int): Boolean {
      val ask = screenAsk ?: return false
      ScreenFrames.fps = fps.coerceIn(1, 30)
      ScreenFrames.maxSide = maxSide.coerceIn(320, 1920)
      ScreenFrames.jpeg = ""
      ScreenFrames.state = "asking"
      runOnUiThread {
        try {
          val mpm = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
          ask.launch(mpm.createScreenCaptureIntent())
        } catch (e: Exception) {
          ScreenFrames.state = "error"
        }
      }
      return true
    }
    @JavascriptInterface fun stop() {
      ScreenFrames.state = "off"
      try { stopService(Intent(this@MainActivity, AquaScreenService::class.java)) } catch (_: Exception) {}
    }
    @JavascriptInterface fun state(): String = ScreenFrames.state
    /** "seq,width,height,base64 JPEG" of a frame newer than `after`, or "". */
    @JavascriptInterface fun frame(after: Int): String {
      val n = ScreenFrames.seq
      if (n <= after) return ""
      return "$n,${ScreenFrames.w},${ScreenFrames.h},${ScreenFrames.jpeg}"
    }
  }

  private fun pushInsets(v: WebView) {
    val json = "{\"l\":${bars[0]},\"t\":${bars[1]},\"r\":${bars[2]},\"b\":${bars[3]},\"w\":${v.width},\"h\":${v.height}}"
    if (json == insetsJson) return
    insetsJson = json
    v.post { v.evaluateJavascript("window.__aquaInsets&&window.__aquaInsets($json)", null) }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    requestLocalNetworkAccess()
    // The answer to the screen-capture prompt: the capture runs in a
    // foreground service of type mediaProjection (required since Android 10,
    // and on 14+ it must start after the user said yes).
    screenAsk = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
      val data = r.data
      if (r.resultCode == Activity.RESULT_OK && data != null) {
        val i = Intent(this, AquaScreenService::class.java).putExtra("code", r.resultCode).putExtra("data", data)
        try {
          if (Build.VERSION.SDK_INT >= 26) startForegroundService(i) else startService(i)
        } catch (e: Exception) {
          ScreenFrames.state = "error"
        }
      } else {
        ScreenFrames.state = "denied"
      }
    }
    // Back / the back gesture. With targetSdk 33+ Android no longer sends
    // the Back key to the WebView (predictive back), so the page's history
    // never saw it and the app simply closed. Now the page decides
    // (ui/src/lib/back.ts, window.__aquaBack): close what's open on top, the
    // chat, another tab… With nothing left, the app goes to the background
    // like Home — it keeps running (notifications, music) instead of exiting.
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val w = web
        if (w == null) { moveTaskToBack(true); return }
        w.evaluateJavascript("(function(){try{return !!(window.__aquaBack&&window.__aquaBack())}catch(e){return false}})()") { r ->
          if (r != "true") moveTaskToBack(true)
        }
      }
    })
  }

  // Let the page's viewport tag set the zoom: Settings → Appearance →
  // Interface scale changes its initial-scale (theme.ts), like a browser's
  // page zoom. The tag keeps width=device-width otherwise, so nothing else
  // changes.
  override fun onWebViewCreate(webView: WebView) {
    web = webView
    webView.addJavascriptInterface(InsetsBridge(), "AquaInsets")
    webView.addJavascriptInterface(ScreenBridge(), "AquaScreen")
    ViewCompat.setOnApplyWindowInsetsListener(webView) { v, insets ->
      val b = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      bars = intArrayOf(b.left, b.top, b.right, b.bottom)
      pushInsets(v as WebView)
      // The WebView still gets them too (its own safe-area handling).
      ViewCompat.onApplyWindowInsets(v, insets)
    }
    webView.addOnLayoutChangeListener { v, _, _, _, _, _, _, _, _ -> pushInsets(v as WebView) }
    webView.settings.useWideViewPort = true
    webView.settings.loadWithOverviewMode = false
    // The system's font size made every text bigger than the layout around
    // it (rows wrapped word by word, buttons ran off the screen). The app
    // has its own text size and interface scale (Appearance) instead.
    webView.settings.textZoom = 100
  }

  // Android 17 (targetSdk 37) blocks connections to local / private addresses
  // - a self-hosted relay at 192.168.x.x, the emulator's 10.0.2.2 - until the
  // user grants "local network" access (NEARBY_DEVICES permission group).
  // Internet relays are unaffected.
  private fun requestLocalNetworkAccess() {
    if (Build.VERSION.SDK_INT < 37) return
    val perm = "android.permission.ACCESS_LOCAL_NETWORK"
    if (checkSelfPermission(perm) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(arrayOf(perm), 4242)
    }
  }
}

/** The latest frame of the shared screen, for ScreenBridge.frame. */
object ScreenFrames {
  @Volatile var state: String = "off" // off | asking | on | denied | error
  @Volatile var jpeg: String = ""
  @Volatile var seq: Int = 0
  @Volatile var w: Int = 0
  @Volatile var h: Int = 0
  @Volatile var fps: Int = 10
  @Volatile var maxSide: Int = 1280
}

/** Captures the screen while a call shares it (MainActivity.ScreenBridge). */
class AquaScreenService : Service() {
  private var proj: MediaProjection? = null
  private var display: VirtualDisplay? = null
  private var reader: ImageReader? = null
  private var worker: HandlerThread? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    val note: Notification = if (Build.VERSION.SDK_INT >= 26) {
      nm.createNotificationChannel(NotificationChannel("aqua_screen", "Screen sharing", NotificationManager.IMPORTANCE_LOW))
      Notification.Builder(this, "aqua_screen")
        .setSmallIcon(applicationInfo.icon)
        .setContentTitle("Aquamarine")
        .setContentText("Your screen is shared in a call")
        .setOngoing(true)
        .build()
    } else {
      @Suppress("DEPRECATION")
      Notification.Builder(this)
        .setSmallIcon(applicationInfo.icon)
        .setContentTitle("Aquamarine")
        .setContentText("Your screen is shared in a call")
        .setOngoing(true)
        .build()
    }
    if (Build.VERSION.SDK_INT >= 29) {
      startForeground(7342, note, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION)
    } else {
      startForeground(7342, note)
    }
    val code = intent?.getIntExtra("code", 0) ?: 0
    val data: Intent? = if (Build.VERSION.SDK_INT >= 33) {
      intent?.getParcelableExtra("data", Intent::class.java)
    } else {
      @Suppress("DEPRECATION")
      intent?.getParcelableExtra("data")
    }
    if (proj != null) {
      // Still capturing from before: keep that one.
      ScreenFrames.state = "on"
      return START_NOT_STICKY
    }
    if (data == null) {
      ScreenFrames.state = "error"
      stopSelf()
      return START_NOT_STICKY
    }
    try {
      begin(code, data)
      ScreenFrames.state = "on"
    } catch (e: Exception) {
      ScreenFrames.state = "error"
      stopSelf()
    }
    return START_NOT_STICKY
  }

  private fun begin(code: Int, data: Intent) {
    val mpm = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    val p = mpm.getMediaProjection(code, data) ?: throw IllegalStateException("no projection")
    proj = p
    val t = HandlerThread("aqua-screen")
    t.start()
    worker = t
    val handler = Handler(t.looper)
    // Android 14 wants a callback before the virtual display; the user can
    // also stop sharing from the status bar.
    p.registerCallback(object : MediaProjection.Callback() {
      override fun onStop() {
        ScreenFrames.state = "off"
        stopSelf()
      }
    }, handler)
    val m = resources.displayMetrics
    val big = maxOf(m.widthPixels, m.heightPixels).toFloat()
    val k = minOf(1f, ScreenFrames.maxSide / big)
    val w = maxOf(2, ((m.widthPixels * k).toInt() / 2) * 2)
    val h = maxOf(2, ((m.heightPixels * k).toInt() / 2) * 2)
    val r = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 2)
    reader = r
    val out = ByteArrayOutputStream()
    var last = 0L
    r.setOnImageAvailableListener({ rd ->
      val img = rd.acquireLatestImage()
      if (img != null) {
        try {
          val now = SystemClock.uptimeMillis()
          if (now - last >= 1000L / ScreenFrames.fps) {
            last = now
            val plane = img.planes[0]
            val rowPx = plane.rowStride / plane.pixelStride
            val bmp = Bitmap.createBitmap(rowPx, h, Bitmap.Config.ARGB_8888)
            bmp.copyPixelsFromBuffer(plane.buffer)
            val pic = if (rowPx != w) Bitmap.createBitmap(bmp, 0, 0, w, h) else bmp
            out.reset()
            pic.compress(Bitmap.CompressFormat.JPEG, 70, out)
            if (pic !== bmp) pic.recycle()
            bmp.recycle()
            ScreenFrames.jpeg = Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
            ScreenFrames.w = w
            ScreenFrames.h = h
            ScreenFrames.seq = ScreenFrames.seq + 1
          }
        } catch (_: Exception) {
        } finally {
          img.close()
        }
      }
    }, handler)
    display = p.createVirtualDisplay("aqua-screen", w, h, m.densityDpi, DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, r.surface, null, handler)
  }

  override fun onDestroy() {
    try { display?.release() } catch (_: Exception) {}
    try { reader?.close() } catch (_: Exception) {}
    try { proj?.stop() } catch (_: Exception) {}
    worker?.quitSafely()
    display = null; reader = null; proj = null; worker = null
    ScreenFrames.jpeg = ""
    if (ScreenFrames.state == "on") ScreenFrames.state = "off"
    super.onDestroy()
  }
}
