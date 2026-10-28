package app.aquamarine.messenger

import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

// Copied over the Tauri-generated file by android/patch-project.ps1 and CI.
class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    requestLocalNetworkAccess()
  }

  // Let the page's viewport tag set the zoom: Settings → Appearance →
  // Interface scale changes its initial-scale (theme.ts), like a browser's
  // page zoom. The tag keeps width=device-width otherwise, so nothing else
  // changes.
  override fun onWebViewCreate(webView: WebView) {
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
