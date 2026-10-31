@file:Suppress("unused", "UNUSED_PARAMETER")

package android.webkit

import okhttp3.Cookie
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.util.concurrent.ConcurrentHashMap

fun interface ValueCallback<T> {
    fun onReceiveValue(value: T)
}

/**
 * Almacén de cookies compartido. Las extensiones lo usan para leer o fijar cookies a mano
 * (`CookieManager.getInstance().setCookie(...)`) y el cliente HTTP del host las lee de aquí,
 * como en Android, donde el cookie jar de Mihon está respaldado por el CookieManager de WebView.
 */
class CookieManager private constructor() {
    private val store = ConcurrentHashMap<String, MutableList<Cookie>>()
    private var accept = true

    fun setAcceptCookie(accept: Boolean) {
        this.accept = accept
    }

    fun acceptCookie(): Boolean = accept

    fun setAcceptThirdPartyCookies(webview: Any?, accept: Boolean) {}

    fun setCookie(url: String, value: String) {
        val httpUrl = url.toHttpUrlOrNull() ?: return
        Cookie.parse(httpUrl, value)?.let { save(httpUrl, listOf(it)) }
    }

    fun setCookie(url: String, value: String, callback: ValueCallback<Boolean>?) {
        setCookie(url, value)
        callback?.onReceiveValue(true)
    }

    /** Las cookies que irían a `url`, en el formato de la cabecera Cookie ("a=1; b=2"), o null. */
    fun getCookie(url: String): String? {
        val httpUrl = url.toHttpUrlOrNull() ?: return null
        return load(httpUrl).takeIf { it.isNotEmpty() }?.joinToString("; ") { "${it.name}=${it.value}" }
    }

    fun hasCookies(): Boolean = store.values.any { it.isNotEmpty() }

    fun removeAllCookies(callback: ValueCallback<Boolean>?) {
        store.clear()
        callback?.onReceiveValue(true)
    }

    fun removeAllCookie() = store.clear()

    fun removeSessionCookies(callback: ValueCallback<Boolean>?) {
        store.values.forEach { list -> synchronized(list) { list.removeAll { !it.persistent } } }
        callback?.onReceiveValue(true)
    }

    fun removeSessionCookie() = removeSessionCookies(null)

    fun removeExpiredCookie() {
        val now = System.currentTimeMillis()
        store.values.forEach { list -> synchronized(list) { list.removeAll { it.expiresAt <= now } } }
    }

    fun flush() {}

    fun save(url: HttpUrl, cookies: List<Cookie>) {
        if (!accept) return
        for (cookie in cookies) {
            val list = store.getOrPut(cookie.domain) { mutableListOf() }
            synchronized(list) {
                list.removeAll { it.name == cookie.name && it.path == cookie.path }
                list.add(cookie)
            }
        }
    }

    fun load(url: HttpUrl): List<Cookie> {
        val now = System.currentTimeMillis()
        return store.values.flatMap { list -> synchronized(list) { list.filter { it.expiresAt > now && it.matches(url) } } }
    }

    companion object {
        private val instance = CookieManager()

        @JvmStatic
        fun getInstance(): CookieManager = instance
    }
}
