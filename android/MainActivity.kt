package app.aquamarine.messenger

import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
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
