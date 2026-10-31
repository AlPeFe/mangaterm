@file:Suppress("unused")

package android.util

object Log {
    private fun log(level: String, tag: String?, msg: String?, tr: Throwable? = null): Int {
        System.err.println("[$level/$tag] $msg")
        tr?.printStackTrace()
        return 0
    }

    @JvmStatic fun v(tag: String?, msg: String?) = log("V", tag, msg)
    @JvmStatic fun d(tag: String?, msg: String?) = log("D", tag, msg)
    @JvmStatic fun i(tag: String?, msg: String?) = log("I", tag, msg)
    @JvmStatic fun w(tag: String?, msg: String?) = log("W", tag, msg)
    @JvmStatic fun w(tag: String?, msg: String?, tr: Throwable?) = log("W", tag, msg, tr)
    @JvmStatic fun e(tag: String?, msg: String?) = log("E", tag, msg)
    @JvmStatic fun e(tag: String?, msg: String?, tr: Throwable?) = log("E", tag, msg, tr)
    @JvmStatic fun wtf(tag: String?, msg: String?) = log("WTF", tag, msg)
}

object Base64 {
    const val DEFAULT = 0
    const val NO_PADDING = 1
    const val NO_WRAP = 2
    const val CRLF = 4
    const val URL_SAFE = 8

    @JvmStatic
    fun decode(str: String, flags: Int): ByteArray = decode(str.toByteArray(), flags)

    @JvmStatic
    fun decode(input: ByteArray, flags: Int): ByteArray {
        // Android tolera espacios/saltos de línea y padding ausente
        val clean = String(input).filterNot { it.isWhitespace() }
        val decoder = if (flags and URL_SAFE != 0) java.util.Base64.getUrlDecoder() else java.util.Base64.getDecoder()
        return decoder.decode(clean.trimEnd('=').let { it + "=".repeat((4 - it.length % 4) % 4) })
    }

    @JvmStatic
    fun encodeToString(input: ByteArray, flags: Int): String = String(encode(input, flags))

    @JvmStatic
    fun encode(input: ByteArray, flags: Int): ByteArray {
        var encoder = if (flags and URL_SAFE != 0) java.util.Base64.getUrlEncoder() else java.util.Base64.getEncoder()
        if (flags and NO_PADDING != 0) encoder = encoder.withoutPadding()
        val out = encoder.encode(input)
        return if (flags and NO_WRAP != 0) out else out + '\n'.code.toByte()
    }
}

/** Caché LRU con tamaño máximo (por defecto cada entrada cuenta 1, como en Android). */
open class LruCache<K : Any, V : Any>(private var maxSize: Int) {
    private val map = LinkedHashMap<K, V>(0, 0.75f, true)
    private var size = 0

    @Synchronized
    fun get(key: K): V? = map[key] ?: create(key)?.also { put(key, it) }

    @Synchronized
    fun put(key: K, value: V): V? {
        val previous = map.put(key, value)
        size += sizeOf(key, value) - (previous?.let { sizeOf(key, it) } ?: 0)
        trimToSize(maxSize)
        return previous
    }

    @Synchronized
    fun remove(key: K): V? = map.remove(key)?.also { size -= sizeOf(key, it) }

    @Synchronized
    fun trimToSize(maxSize: Int) {
        val it = map.entries.iterator()
        while (size > maxSize && it.hasNext()) {
            val (k, v) = it.next()
            it.remove()
            size -= sizeOf(k, v)
            entryRemoved(true, k, v, null)
        }
    }

    fun evictAll() = trimToSize(-1)

    fun resize(maxSize: Int) {
        this.maxSize = maxSize
        trimToSize(maxSize)
    }

    @Synchronized
    fun size(): Int = size

    fun maxSize(): Int = maxSize

    @Synchronized
    fun snapshot(): Map<K, V> = LinkedHashMap(map)

    protected open fun sizeOf(key: K, value: V): Int = 1

    protected open fun create(key: K): V? = null

    protected open fun entryRemoved(evicted: Boolean, key: K, oldValue: V, newValue: V?) {}
}
