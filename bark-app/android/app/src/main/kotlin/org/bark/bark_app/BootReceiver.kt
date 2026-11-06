package org.bark.bark_app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

// Starts the link's service when the phone has started, if the app was
// looking for or connected to its paired node. BOOT_COMPLETED comes after
// the person first unlocks the phone: before that the app's files, the
// identity among them, are still encrypted, and the app could not run.
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) LinkService.boot(context)
    }
}
