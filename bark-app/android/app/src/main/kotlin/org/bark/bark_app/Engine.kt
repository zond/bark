package org.bark.bark_app

import android.content.Context
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.FlutterEngineCache
import io.flutter.embedding.engine.dart.DartExecutor
import io.flutter.plugin.common.MethodChannel

// One Dart engine for the process, which holds the session with the node.
// The window attaches to it and leaves it running when it goes; the link's
// service runs it without a window when the phone starts. Plugins are
// registered when the engine is made.
object Engine {
    private const val ID = "main"
    private const val LINK = "org.bark.bark_app/link"

    fun get(context: Context): FlutterEngine {
        val cache = FlutterEngineCache.getInstance()
        cache.get(ID)?.let { return it }
        val app = context.applicationContext
        val engine = FlutterEngine(app)
        // Here and not with the window, as the session asks for the
        // service with no window open.
        MethodChannel(engine.dartExecutor.binaryMessenger, LINK).setMethodCallHandler { call, result ->
            try {
                when (call.method) {
                    "hold" -> LinkService.hold(app, call.argument<String>("title")!!, call.argument<String>("body")!!)
                    "letGo" -> LinkService.letGo(app)
                    "atBoot" -> LinkService.atBoot(app, call.argument<String>("title"), call.argument<String>("body"))
                    else -> return@setMethodCallHandler result.notImplemented()
                }
                result.success(null)
            } catch (e: Exception) {
                result.error(e.javaClass.simpleName, e.message, null)
            }
        }
        cache.put(ID, engine)
        return engine
    }

    // Runs `main` in the engine, unless a window already has.
    fun run(context: Context) {
        val executor = get(context).dartExecutor
        if (!executor.isExecutingDart) executor.executeDartEntrypoint(DartExecutor.DartEntrypoint.createDefault())
    }
}
