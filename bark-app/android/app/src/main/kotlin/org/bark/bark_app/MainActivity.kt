package org.bark.bark_app

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
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
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, BROWSER).setMethodCallHandler { call, result ->
            when (call.method) {
                "open" -> try {
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(call.arguments as String)))
                    result.success(null)
                } catch (e: ActivityNotFoundException) {
                    result.error("no-browser", e.message, null)
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
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, SHARE).setMethodCallHandler { call, result ->
            when (call.method) {
                "share" -> {
                    // The image goes through the app's cache and a content
                    // URI, as the receiving app may not read this app's
                    // files otherwise.
                    val dir = File(cacheDir, "shared").apply { mkdirs() }
                    val file = File(dir, "card.png")
                    file.writeBytes(call.argument<ByteArray>("png")!!)
                    val uri = FileProvider.getUriForFile(this, "$packageName.share", file)
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
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, POWER).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, BROWSER).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, BLUETOOTH).setMethodCallHandler(null)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, SHARE).setMethodCallHandler(null)
    }

    private companion object {
        const val POWER = "org.bark.bark_app/power"
        const val BROWSER = "org.bark.bark_app/browser"
        const val SHARE = "org.bark.bark_app/share"
        const val BLUETOOTH = "org.bark.bark_app/bluetooth"
    }
}
