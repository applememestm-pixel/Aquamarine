package app.aquamarine.messenger

// Home-screen widget: unread chats at a glance (crates/aqua-client-ui/src/
// relay_host.rs → AquaRelayPlugin.updateWidget writes what to show; the app
// sends names and counts, and message text only when the user allowed it in
// Settings). Copied next to MainActivity by CI and android/patch-project.sh,
// with its layout from android/widget/res.

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.view.View
import android.widget.RemoteViews
import org.json.JSONObject

class AquaWidget : AppWidgetProvider() {
  override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
    for (id in ids) manager.updateAppWidget(id, views(context))
  }

  companion object {
    const val PREFS = "aqua_widget"

    private val ROWS = intArrayOf(R.id.aw_row1, R.id.aw_row2, R.id.aw_row3)
    private val NAMES = intArrayOf(R.id.aw_name1, R.id.aw_name2, R.id.aw_name3)
    private val TEXTS = intArrayOf(R.id.aw_text1, R.id.aw_text2, R.id.aw_text3)
    private val BADGES = intArrayOf(R.id.aw_badge1, R.id.aw_badge2, R.id.aw_badge3)

    /** Save what the app sent and redraw every placed widget. */
    fun store(context: Context, json: String) {
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString("data", json).apply()
      val manager = AppWidgetManager.getInstance(context)
      val ids = manager.getAppWidgetIds(ComponentName(context, AquaWidget::class.java))
      for (id in ids) manager.updateAppWidget(id, views(context))
    }

    fun views(context: Context): RemoteViews {
      val v = RemoteViews(context.packageName, R.layout.aqua_widget)
      val data = try {
        JSONObject(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("data", "{}") ?: "{}")
      } catch (e: Exception) { JSONObject() }
      val unread = data.optInt("unread", 0)
      v.setTextViewText(R.id.aw_title, data.optString("title", "Aquamarine"))
      v.setTextViewText(R.id.aw_count, if (unread > 0) unread.toString() else "")
      v.setViewVisibility(R.id.aw_count, if (unread > 0) View.VISIBLE else View.GONE)
      val rows = data.optJSONArray("rows")
      var shown = 0
      for (i in ROWS.indices) {
        val r = rows?.optJSONObject(i)
        if (r == null) { v.setViewVisibility(ROWS[i], View.GONE); continue }
        shown++
        v.setViewVisibility(ROWS[i], View.VISIBLE)
        v.setTextViewText(NAMES[i], r.optString("n"))
        val text = r.optString("p")
        v.setTextViewText(TEXTS[i], text)
        v.setViewVisibility(TEXTS[i], if (text.isEmpty()) View.GONE else View.VISIBLE)
        val u = r.optInt("u", 0)
        v.setTextViewText(BADGES[i], if (u > 99) "99+" else u.toString())
        v.setViewVisibility(BADGES[i], if (u > 0) View.VISIBLE else View.GONE)
      }
      v.setTextViewText(R.id.aw_empty, data.optString("empty", "No new messages"))
      v.setViewVisibility(R.id.aw_empty, if (shown == 0) View.VISIBLE else View.GONE)
      // A tap anywhere opens the app.
      context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { launch ->
        val pi = PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        v.setOnClickPendingIntent(R.id.aw_root, pi)
      }
      return v
    }
  }
}
