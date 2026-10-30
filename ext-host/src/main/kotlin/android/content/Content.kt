@file:Suppress("unused", "UNUSED_PARAMETER")

// Shims de las clases de Android que usan las extensiones.
// Solo replican nombres y firmas: lo justo para que el bytecode de las extensiones enlace en una JVM normal.
// La lista sale de analizar los JARs (ver NOTES.md).

package android.content

import android.net.Uri
import mangaterm.host.FilePrefs
import mangaterm.host.HostDirs
import java.io.File

abstract class Context {
    open fun getSharedPreferences(name: String, mode: Int): SharedPreferences = FilePrefs.of(name)
    open fun getCacheDir(): File = HostDirs.cache
    open fun getFilesDir(): File = HostDirs.data
    open fun getPackageName(): String = "app.mangaterm"
    open fun getApplicationContext(): Context = this
    open fun startActivity(intent: Intent): Unit = throw ActivityNotFoundException()
}

interface SharedPreferences {
    fun getAll(): Map<String, *>
    fun getString(key: String, defValue: String?): String?
    fun getStringSet(key: String, defValues: Set<String>?): Set<String>?
    fun getInt(key: String, defValue: Int): Int
    fun getLong(key: String, defValue: Long): Long
    fun getFloat(key: String, defValue: Float): Float
    fun getBoolean(key: String, defValue: Boolean): Boolean
    fun contains(key: String): Boolean
    fun edit(): Editor

    interface Editor {
        fun putString(key: String, value: String?): Editor
        fun putStringSet(key: String, values: Set<String>?): Editor
        fun putInt(key: String, value: Int): Editor
        fun putLong(key: String, value: Long): Editor
        fun putFloat(key: String, value: Float): Editor
        fun putBoolean(key: String, value: Boolean): Editor
        fun remove(key: String): Editor
        fun clear(): Editor
        fun commit(): Boolean
        fun apply()
    }
}

open class Intent {
    private var action: String? = null
    private var data: Uri? = null
    private val extras = mutableMapOf<String, Any?>()

    constructor()
    constructor(action: String?) { this.action = action }

    fun setAction(action: String?): Intent = apply { this.action = action }
    fun getAction(): String? = action
    fun getData(): Uri? = data
    fun setData(data: Uri?): Intent = apply { this.data = data }
    fun putExtra(name: String, value: String?): Intent = apply { extras[name] = value }
    fun getStringExtra(name: String): String? = extras[name] as? String
}

class ActivityNotFoundException(message: String? = null) : RuntimeException(message)
