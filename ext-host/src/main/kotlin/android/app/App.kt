@file:Suppress("unused", "UNUSED_PARAMETER")

package android.app

import android.content.Context
import android.content.Intent
import android.os.Bundle

open class Application : Context()

/** Las extensiones solo la usan en UrlActivity (deep links), que el host nunca ejecuta. */
open class Activity : Context() {
    private var intent = Intent()

    open fun onCreate(savedInstanceState: Bundle?) {}
    open fun getIntent(): Intent = intent
    open fun setIntent(newIntent: Intent) { intent = newIntent }
    open fun finish() {}
}
