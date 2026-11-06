package org.bark.bark_app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.FlutterEngineCache
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    // One engine for the process, kept when the activity goes: the Dart
    // session in it holds the link to the node, which the foreground
    // service keeps running after the person closes the app's window.
    // Plugins are registered when the engine is made.
    override fun provideFlutterEngine(context: Context): FlutterEngine {
        val cache = FlutterEngineCache.getInstance()
        return cache.get(ENGINE) ?: FlutterEngine(context.applicationContext).also {
            cache.put(ENGINE, it)
        }
    }

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
        const val ENGINE = "main"
        const val POWER = "org.bark.bark_app/power"
    }
}
