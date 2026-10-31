package hantho.host

import android.app.Application
import android.content.SharedPreferences
import eu.kanade.tachiyomi.network.NetworkHelper
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.protobuf.ProtoBuf
import uy.kohesive.injekt.Injekt
import uy.kohesive.injekt.api.addSingleton
import uy.kohesive.injekt.api.addSingletonFactory
import java.io.File
import java.util.concurrent.ConcurrentHashMap

object HostDirs {
    val root: File = (System.getenv("HANTHO_HOME")?.let(::File) ?: File(System.getProperty("user.home"), ".hantho"))
    val data = File(root, "data").apply { mkdirs() }
    val cache = File(root, "cache").apply { mkdirs() }
    val prefs = File(data, "prefs").apply { mkdirs() }
}

/** Registra en Injekt lo que las extensiones piden con injectLazy()/Injekt.get(). */
object HostEnv {
    val app = Application()

    @OptIn(ExperimentalSerializationApi::class)
    fun init() {
        Injekt.addSingleton(app)
        Injekt.addSingletonFactory { NetworkHelper(app) }
        Injekt.addSingletonFactory {
            Json {
                ignoreUnknownKeys = true
                explicitNulls = false
            }
        }
        Injekt.addSingletonFactory<ProtoBuf> { ProtoBuf }
    }
}

/** SharedPreferences persistidas en un JSON por nombre (data/prefs/<nombre>.json). */
class FilePrefs private constructor(private val file: File) : SharedPreferences {
    private val values = ConcurrentHashMap<String, Any>()

    init {
        if (file.exists()) {
            (Json.parseToJsonElement(file.readText()) as? JsonObject)?.forEach { (k, v) ->
                fromJson(v)?.let { values[k] = it }
            }
        }
    }

    override fun getAll(): Map<String, *> = values.toMap()
    override fun getString(key: String, defValue: String?) = values[key] as? String ?: defValue

    @Suppress("UNCHECKED_CAST")
    override fun getStringSet(key: String, defValues: Set<String>?) = values[key] as? Set<String> ?: defValues
    override fun getInt(key: String, defValue: Int) = (values[key] as? Number)?.toInt() ?: defValue
    override fun getLong(key: String, defValue: Long) = (values[key] as? Number)?.toLong() ?: defValue
    override fun getFloat(key: String, defValue: Float) = (values[key] as? Number)?.toFloat() ?: defValue
    override fun getBoolean(key: String, defValue: Boolean) = values[key] as? Boolean ?: defValue
    override fun contains(key: String) = values.containsKey(key)
    override fun edit(): SharedPreferences.Editor = EditorImpl()

    private fun save() = synchronized(this) {
        file.writeText(JsonObject(values.mapValues { toJson(it.value) }).toString())
    }

    private inner class EditorImpl : SharedPreferences.Editor {
        private val changes = mutableMapOf<String, Any?>()
        private var clear = false

        override fun putString(key: String, value: String?) = apply { changes[key] = value }
        override fun putStringSet(key: String, values: Set<String>?) = apply { changes[key] = values?.toSet() }
        override fun putInt(key: String, value: Int) = apply { changes[key] = value }
        override fun putLong(key: String, value: Long) = apply { changes[key] = value }
        override fun putFloat(key: String, value: Float) = apply { changes[key] = value }
        override fun putBoolean(key: String, value: Boolean) = apply { changes[key] = value }
        override fun remove(key: String) = apply { changes[key] = null }
        override fun clear() = apply { clear = true }

        override fun commit(): Boolean {
            if (clear) values.clear()
            changes.forEach { (k, v) -> if (v == null) values.remove(k) else values[k] = v }
            save()
            return true
        }

        override fun apply() {
            commit()
        }
    }

    companion object {
        private val cache = ConcurrentHashMap<String, FilePrefs>()

        fun of(name: String): FilePrefs = cache.getOrPut(name) {
            FilePrefs(File(HostDirs.prefs, name.replace(Regex("[^\\w.-]"), "_") + ".json"))
        }

        private fun toJson(v: Any?): JsonElement = when (v) {
            is String -> JsonPrimitive(v)
            is Number -> JsonPrimitive(v)
            is Boolean -> JsonPrimitive(v)
            is Set<*> -> JsonArray(v.map { JsonPrimitive(it.toString()) })
            else -> JsonNull
        }

        private fun fromJson(e: JsonElement): Any? = when (e) {
            is JsonArray -> e.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.toSet()
            is JsonPrimitive -> when {
                e.isString -> e.content
                e.booleanOrNull != null -> e.booleanOrNull
                e.longOrNull != null -> e.longOrNull!!.let { if (it in Int.MIN_VALUE..Int.MAX_VALUE) it.toInt() else it }
                else -> e.doubleOrNull?.toFloat()
            }
            else -> null
        }
    }
}
