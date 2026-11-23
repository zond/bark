package org.bark.bark_app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log

// The foreground service that keeps the process, and the Dart session in
// it, running while the app holds the link to a node or looks for its
// paired node. It shows the link's notification and names its channel,
// both in words the session gives in the language spoken, and starts the
// session without a window when nothing else has: at boot, or when Android
// starts the service again after ending the process.
class LinkService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val words = intent?.let { Words.of(it) } ?: Words.kept(prefs(this))
        if (words == null) {
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            val n = notification(this, words)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
            } else {
                startForeground(ID, n)
            }
        } catch (e: RuntimeException) {
            // Android 12 and later refuse a start from the background
            // outside the cases they allow, and 14 and later refuse the
            // connected-device type without a Bluetooth permission.
            Log.w(TAG, "not started: $e")
            stopSelf()
            return START_NOT_STICKY
        }
        running = true
        Engine.run(this)
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    // The notification's title and body, and its channel's name and
    // description.
    data class Words(
        val title: String,
        val body: String,
        val channel: String,
        val about: String,
    ) {
        fun into(i: Intent): Intent = i
            .putExtra(TITLE, title)
            .putExtra(BODY, body)
            .putExtra(CHANNEL_NAME, channel)
            .putExtra(ABOUT, about)

        companion object {
            fun of(i: Intent): Words? = Words(
                i.getStringExtra(TITLE) ?: return null,
                i.getStringExtra(BODY) ?: return null,
                i.getStringExtra(CHANNEL_NAME) ?: return null,
                i.getStringExtra(ABOUT) ?: return null,
            )

            // A build before this one kept no channel words; its English
            // ones, so that such a phone still starts the service at boot.
            fun kept(p: SharedPreferences): Words? = Words(
                p.getString(TITLE, null) ?: return null,
                p.getString(BODY, null) ?: return null,
                p.getString(CHANNEL_NAME, null) ?: "Connection to the node",
                p.getString(ABOUT, null)
                    ?: "Shown while the app keeps the link to a node or looks for it",
            )
        }
    }

    companion object {
        private const val TAG = "bark"
        private const val ID = 1
        private const val CHANNEL = "link"
        private const val TITLE = "title"
        private const val BODY = "body"
        private const val CHANNEL_NAME = "channel"
        private const val ABOUT = "channelAbout"

        private var running = false

        private fun prefs(context: Context) = context.getSharedPreferences("boot", Context.MODE_PRIVATE)

        // Runs the service under a notification of `words`.
        fun hold(context: Context, words: Words) {
            if (running) {
                // Changed in place: Android 12 and later restrict starting
                // a service from the background.
                context.getSystemService(NotificationManager::class.java)
                    .notify(ID, notification(context, words))
                return
            }
            start(context, words.into(Intent(context, LinkService::class.java)))
        }

        fun letGo(context: Context) {
            // A hold that comes before the service is destroyed starts it
            // again rather than changing a notification about to go.
            running = false
            context.stopService(Intent(context, LinkService::class.java))
        }

        // Kept for boot: the notification to start under, or none to
        // start nothing.
        fun atBoot(context: Context, words: Words?) {
            prefs(context).edit()
                .putString(TITLE, words?.title)
                .putString(BODY, words?.body)
                .putString(CHANNEL_NAME, words?.channel)
                .putString(ABOUT, words?.about)
                .apply()
        }

        fun boot(context: Context) {
            if (Words.kept(prefs(context)) == null) return
            try {
                // Android 12 and later allow a foreground service to start
                // from the receiver of BOOT_COMPLETED ("Restrictions on
                // starting a foreground service from the background",
                // exemptions); Android 15 bars only other service types.
                start(context, Intent(context, LinkService::class.java))
            } catch (e: RuntimeException) {
                Log.w(TAG, "not started at boot: $e")
            }
        }

        private fun start(context: Context, intent: Intent) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        private fun notification(context: Context, words: Words): Notification {
            val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                // Low importance makes no sound: it is there as long as
                // the link or the search, and Android requires it of a
                // foreground service.
                context.getSystemService(NotificationManager::class.java).createNotificationChannel(
                    // Made again at each notification, which renames it
                    // when the phone's language changed.
                    NotificationChannel(CHANNEL, words.channel, NotificationManager.IMPORTANCE_LOW).apply {
                        description = words.about
                    },
                )
                Notification.Builder(context, CHANNEL)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(context).setPriority(Notification.PRIORITY_LOW)
            }
            val open = PendingIntent.getActivity(
                context,
                0,
                context.packageManager.getLaunchIntentForPackage(context.packageName),
                PendingIntent.FLAG_IMMUTABLE,
            )
            return builder
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(words.title)
                .setContentText(words.body)
                .setOngoing(true)
                .setShowWhen(false)
                .setContentIntent(open)
                .build()
        }
    }
}
