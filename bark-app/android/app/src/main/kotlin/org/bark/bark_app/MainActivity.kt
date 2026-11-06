package org.bark.bark_app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

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
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, POWER).setMethodCallHandler(null)
    }

    private companion object {
        const val POWER = "org.bark.bark_app/power"
    }
}
