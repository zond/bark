package org.bark.bark_app

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.view.WindowManager
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import androidx.core.content.FileProvider
import java.io.File

class MainActivity : FlutterActivity() {
    // The process's one engine (Engine.kt), kept when the window goes, and
    // already running when the app started with the phone: the window then
    // shows the session as it stands.
    override fun provideFlutterEngine(context: Context): FlutterEngine = Engine.get(context)

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        // A window made while a secret is shown, as after a rotation, is
        // secured too.
        secure(secret)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, WINDOW).setMethodCallHandler { call, result ->
            when (call.method) {
                // While the seed or a password is on screen: no screenshot,
                // recording or recents thumbnail shows it.
                "secure" -> {
                    secret = call.arguments as Boolean
                    secure(secret)
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, POWER).setMethodCallHandler { call, result ->
            when (call.method) {
                "exempt" -> result.success(
                    getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(packageName),
                )
                "askExempt" -> {
                    // Allowed for an app whose purpose needs a connection
                    // kept while the phone sleeps.
                    startActivity(
                        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                            .setData(Uri.parse("package:$packageName")),
                    )
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, INSTALLER).setMethodCallHandler { call, result ->
            when (call.method) {
                // Through a content URI, as the installer may not read this
                // app's files otherwise. The first time, it sends the
                // person to allow this app to install apps.
                "install" -> try {
                    val file = File(call.arguments as String)
                    val uri = FileProvider.getUriForFile(this, "$packageName.files", file)
                    startActivity(
                        Intent(Intent.ACTION_VIEW)
                            .setDataAndType(uri, "application/vnd.android.package-archive")
                            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
                    )
                    result.success(null)
                } catch (e: ActivityNotFoundException) {
                    result.error("no-installer", e.message, null)
                } catch (e: IllegalArgumentException) {
                    // A path outside the provider's folders.
                    result.error("not-provided", e.message, null)
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, BLUETOOTH).setMethodCallHandler { call, result ->
            when (call.method) {
                // Where the person forgets a node whose bond the phone
                // could not remove itself.
                "settings" -> try {
                    startActivity(Intent(Settings.ACTION_BLUETOOTH_SETTINGS))
                    result.success(null)
                } catch (e: ActivityNotFoundException) {
                    result.error("no-settings", e.message, null)
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, WEB).setMethodCallHandler { call, result ->
            when (call.method) {
                // A page in the phone's browser, or in the app that takes its
                // links: the manual, the users' channel.
                "open" -> try {
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(call.arguments as String)))
                    result.success(null)
                } catch (e: ActivityNotFoundException) {
                    result.error("no-browser", e.message, null)
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, SHARE).setMethodCallHandler { call, result ->
            when (call.method) {
                "share" -> {
                    // The image goes through the app's cache and a content
                    // URI, as the receiving app may not read this app's
                    // files otherwise.
                    val dir = File(cacheDir, "shared").apply { mkdirs() }
                    val file = File(dir, "card.png")
                    file.writeBytes(call.argument<ByteArray>("png")!!)
                    val uri = FileProvider.getUriForFile(this, "$packageName.files", file)
                    val send = Intent(Intent.ACTION_SEND)
                        .setType("image/png")
                        .putExtra(Intent.EXTRA_TEXT, call.argument<String>("text"))
                        .putExtra(Intent.EXTRA_STREAM, uri)
                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    send.clipData = ClipData.newRawUri("", uri)
                    startActivity(Intent.createChooser(send, null))
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, FILES).setMethodCallHandler { call, result ->
            when (call.method) {
                // Android's own save dialog, which the file picker the app
                // uses offers only for opening. The bytes wait here for
                // the place the person picks.
                "save" -> {
                    if (saving != null) {
                        result.error("busy", "a save is under way", null)
                        return@setMethodCallHandler
                    }
                    saving = result
                    savingBytes = call.argument<ByteArray>("bytes")
                    try {
                        startActivityForResult(
                            Intent(Intent.ACTION_CREATE_DOCUMENT)
                                .addCategory(Intent.CATEGORY_OPENABLE)
                                .setType("application/octet-stream")
                                .putExtra(Intent.EXTRA_TITLE, call.argument<String>("name")),
                            SAVE_REQUEST,
                        )
                    } catch (e: ActivityNotFoundException) {
                        saving = null
                        savingBytes = null
                        result.error("no-dialog", e.message, null)
                    }
                }
                else -> result.notImplemented()
            }
        }
    }

    private fun secure(on: Boolean) {
        if (on) {
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
    }

    private var saving: MethodChannel.Result? = null
    private var savingBytes: ByteArray? = null

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode != SAVE_REQUEST) {
            super.onActivityResult(requestCode, resultCode, data)
            return
        }
        val result = saving ?: return
        val bytes = savingBytes
        saving = null
        savingBytes = null
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null || bytes == null) {
            result.success(false)
            return
        }
        try {
            // "wt" truncates a file the person chose to replace.
            val out = contentResolver.openOutputStream(uri, "wt")
            if (out == null) {
                result.error("no-stream", "no stream for $uri", uri.toString())
                return
            }
            out.use { it.write(bytes) }
            result.success(true)
        } catch (e: Exception) {
            // Not found, refused by its provider, or a failed write.
            result.error("write", e.message, null)
        }
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, WINDOW).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, POWER).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, INSTALLER).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, BLUETOOTH).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, WEB).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, SHARE).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, FILES).setMethodCallHandler(null)
    }

    private companion object {
        // Kept for the process, as the engine outlives the window.
        var secret = false

        const val WINDOW = "org.bark.bark_app/window"
        const val POWER = "org.bark.bark_app/power"
        const val INSTALLER = "org.bark.bark_app/installer"
        const val SHARE = "org.bark.bark_app/share"
        const val BLUETOOTH = "org.bark.bark_app/bluetooth"
        const val FILES = "org.bark.bark_app/files"
        const val WEB = "org.bark.bark_app/web"

        // The save dialog's answer; no plugin the app uses takes this code.
        const val SAVE_REQUEST = 0x6261
    }
}
