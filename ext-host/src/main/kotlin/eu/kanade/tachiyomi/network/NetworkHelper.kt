package eu.kanade.tachiyomi.network

import android.content.Context
import okhttp3.Cache
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Response
import java.io.File
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

class NetworkHelper(context: Context) {
    val client: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .callTimeout(2, TimeUnit.MINUTES)
        .cookieJar(MemoryCookieJar())
        .cache(Cache(File(context.getCacheDir(), "network_cache"), 50L * 1024 * 1024))
        // keiyoushi.source.KeiSource exige estos tres por nombre de clase (y que NO haya BrotliInterceptor:
        // agrega su propio CompressionInterceptor)
        .addInterceptor(UncaughtExceptionInterceptor())
        .addInterceptor(UserAgentInterceptor())
        .addInterceptor(CloudflareInterceptor())
        .build()

    companion object {
        const val DEFAULT_USER_AGENT =
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36"
    }
}

class HttpException(val code: Int) : IllegalStateException("HTTP error $code")

class JavaScriptEngine(@Suppress("UNUSED_PARAMETER") context: Context) {
    suspend fun <T> evaluate(script: String): T = throw UnsupportedOperationException("JavaScriptEngine no soportado en mangaterm")
}

/** Convierte excepciones no-IO en IOException para que OkHttp no tire abajo el hilo (como en Mihon). */
class UncaughtExceptionInterceptor : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response = try {
        chain.proceed(chain.request())
    } catch (e: Exception) {
        if (e is IOException) throw e else throw IOException(e)
    }
}

class UserAgentInterceptor : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (!request.header("User-Agent").isNullOrBlank()) return chain.proceed(request)
        return chain.proceed(request.newBuilder().header("User-Agent", NetworkHelper.DEFAULT_USER_AGENT).build())
    }
}

/** Sin WebView no podemos resolver el desafío de Cloudflare: lo detectamos y damos un error claro. */
class CloudflareInterceptor : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val response = chain.proceed(chain.request())
        val challenged = response.code in setOf(403, 503) &&
            (
                response.header("cf-mitigated") == "challenge" ||
                    response.peekBody(64 * 1024).string().contains("challenge-platform")
                )
        if (challenged) {
            response.close()
            throw IOException("Cloudflare pide verificación (sin WebView no se puede resolver)")
        }
        return response
    }
}

/** Cookies en memoria por host; alcanza para sesiones simples. */
private class MemoryCookieJar : CookieJar {
    private val store = ConcurrentHashMap<String, MutableList<Cookie>>()

    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        val list = store.getOrPut(url.host) { mutableListOf() }
        synchronized(list) {
            list.removeAll { old -> cookies.any { it.name == old.name } }
            list.addAll(cookies)
        }
    }

    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val now = System.currentTimeMillis()
        return store.entries
            .filter { (host, _) -> url.host == host || url.host.endsWith(".$host") }
            .flatMap { (_, list) -> synchronized(list) { list.filter { it.expiresAt > now && it.matches(url) } } }
    }
}
