// Purrzip by GmCat. Copyright (C) 2026 GmCat. AGPL-3.0-or-later.
// Android glue: the "all files" storage permission (so Purrzip can read archives and
// write the unzipped files where you choose) and "Open with Purrzip" from file managers.
package io.gmcat.purrzip

import android.Manifest
import android.annotation.SuppressLint
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import org.json.JSONArray
import java.io.File

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private val pending = mutableListOf<String>()

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    intent?.let { handleIntent(it, false) }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    handleIntent(intent, true)
  }

  @SuppressLint("JavascriptInterface", "SetJavaScriptEnabled")
  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    webView.addJavascriptInterface(Bridge(), "PurrzipAndroid")
    // Back steps back inside Purrzip (closes a dialog, returns to the start); only leaves from the start screen.
    onBackPressedDispatcher.addCallback(this, object : androidx.activity.OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        webView.evaluateJavascript("window.pzBack ? window.pzBack() : false") { r ->
          if (r != "true") {
            isEnabled = false
            onBackPressedDispatcher.onBackPressed()
            isEnabled = true
          }
        }
      }
    })
  }

  inner class Bridge {
    @JavascriptInterface
    fun hasAllFiles(): Boolean = hasStorage()

    @JavascriptInterface
    fun requestAllFiles() {
      runOnUiThread {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
          try {
            startActivity(Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName")))
          } catch (e: Exception) {
            startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
          }
        } else {
          ActivityCompat.requestPermissions(this@MainActivity,
            arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE), 7)
        }
      }
    }

    @JavascriptInterface
    fun takeStartupPaths(): String {
      synchronized(pending) {
        val a = JSONArray(pending)
        pending.clear()
        return a.toString()
      }
    }
  }

  private fun hasStorage(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
    else ContextCompat.checkSelfPermission(this, Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

  private fun handleIntent(intent: Intent, running: Boolean) {
    if (intent.action != Intent.ACTION_VIEW && intent.action != Intent.ACTION_SEND) return
    val uri: Uri = intent.data ?: (if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
      else @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_STREAM)) ?: return
    Thread {
      val path = resolve(uri) ?: return@Thread
      val wv = webView
      if (running && wv != null) {
        val js = "window.pzOpenPaths && window.pzOpenPaths(" + JSONArray(listOf(path)).toString() + ")"
        runOnUiThread { wv.evaluateJavascript(js, null) }
      } else {
        synchronized(pending) { pending.add(path) }
      }
    }.start()
  }

  // A real file path when Android tells us where the file lives; otherwise a private copy.
  private fun resolve(uri: Uri): String? {
    try {
      if (uri.scheme == "file") return uri.path
      if (hasStorage() && DocumentsContract.isDocumentUri(this, uri) && uri.authority == "com.android.externalstorage.documents") {
        val parts = DocumentsContract.getDocumentId(uri).split(":", limit = 2)
        if (parts.size == 2) {
          val base = if (parts[0] == "primary") Environment.getExternalStorageDirectory().path else "/storage/" + parts[0]
          val f = File(base, parts[1])
          if (f.canRead()) return f.path
        }
      }
      var name = "archive"
      contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) c.getString(0)?.let { name = it.replace('/', '_') }
      }
      val dir = File(cacheDir, "incoming/" + System.currentTimeMillis())
      dir.mkdirs()
      val out = File(dir, name)
      contentResolver.openInputStream(uri)?.use { input -> out.outputStream().use { input.copyTo(it, 1 shl 20) } } ?: return null
      return out.path
    } catch (e: Exception) {
      return null
    }
  }
}
