package android.net

import java.net.URI

class Uri private constructor(private val raw: String) {
    private val parsed by lazy { runCatching { URI(raw) }.getOrNull() }

    fun getScheme(): String? = parsed?.scheme
    fun getHost(): String? = parsed?.host
    fun getPath(): String? = parsed?.path
    fun getQuery(): String? = parsed?.query
    fun getPathSegments(): List<String> = getPath()?.split('/')?.filter { it.isNotEmpty() } ?: emptyList()
    fun getLastPathSegment(): String? = getPathSegments().lastOrNull()

    fun getQueryParameter(key: String): String? = getQuery()
        ?.split('&')
        ?.map { it.split('=', limit = 2) }
        ?.firstOrNull { it[0] == key }
        ?.getOrNull(1)
        ?.let { java.net.URLDecoder.decode(it, Charsets.UTF_8) }

    override fun toString() = raw
    override fun equals(other: Any?) = other is Uri && other.raw == raw
    override fun hashCode() = raw.hashCode()

    companion object {
        @JvmField
        val EMPTY = Uri("")

        @JvmStatic
        fun parse(uriString: String): Uri = Uri(uriString)
    }
}
