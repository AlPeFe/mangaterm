package hantho.host

import eu.kanade.tachiyomi.network.awaitSuccess
import eu.kanade.tachiyomi.source.CatalogueSource
import eu.kanade.tachiyomi.source.model.FilterList
import eu.kanade.tachiyomi.source.model.MangasPage
import eu.kanade.tachiyomi.source.model.Page
import eu.kanade.tachiyomi.source.model.SChapter
import eu.kanade.tachiyomi.source.model.SManga
import eu.kanade.tachiyomi.source.online.HttpSource
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import java.io.File
import java.io.FileDescriptor
import java.io.FileOutputStream
import java.io.PrintStream
import kotlin.system.exitProcess

/*
 * Protocolo: una línea JSON por mensaje en stdin/stdout.
 *   -> {"id":1,"op":"search","source":123,"query":"grand blue","page":1}
 *   <- {"id":1,"ok":true,"result":{...}}   |   {"id":1,"ok":false,"error":"..."}
 * Los pedidos se atienden en paralelo; las respuestas pueden llegar desordenadas (se emparejan por id).
 * Todo lo que no es protocolo (logs, println de extensiones) va a stderr.
 */

private val protocolOut = PrintStream(FileOutputStream(FileDescriptor.out), true, Charsets.UTF_8)

fun main() {
    System.setOut(System.err)
    HostEnv.init()

    val requests = SupervisorJob()
    val scope = CoroutineScope(requests + Dispatchers.IO + CoroutineExceptionHandler { _, e -> e.printStackTrace() })
    val input = System.`in`.bufferedReader(Charsets.UTF_8)
    runBlocking {
        while (true) {
            val line = input.readLine() ?: break
            if (line.isBlank()) continue
            scope.launch { handle(line) }
        }
        // stdin cerrado: terminar de responder lo que ya estaba en curso antes de salir
        requests.children.toList().joinAll()
    }
    // Los hilos de OkHttp no son daemon, así que salimos explícitamente
    exitProcess(0)
}

private suspend fun handle(line: String) {
    var id: JsonElement = JsonNull
    val response = try {
        val req = Json.parseToJsonElement(line).jsonObject
        id = req["id"] ?: JsonNull
        buildJsonObject {
            put("id", id)
            put("ok", true)
            put("result", dispatch(req))
        }
    } catch (e: Throwable) {
        System.err.println("error en pedido $id: $e")
        e.printStackTrace()
        buildJsonObject {
            put("id", id)
            put("ok", false)
            put("error", e.toString())
        }
    }
    synchronized(protocolOut) { protocolOut.println(response.toString()) }
}

private suspend fun dispatch(req: JsonObject): JsonElement {
    fun str(key: String) = req[key]?.jsonPrimitive?.contentOrNull ?: error("Falta '$key'")
    fun page() = req["page"]?.jsonPrimitive?.intOrNull ?: 1
    fun source() = Loader.source(req["source"]?.jsonPrimitive?.long ?: error("Falta 'source'"))

    return when (val op = str("op")) {
        "ping" -> JsonPrimitive("pong")
        "load" -> {
            val ext = Loader.load(str("jar"))
            buildJsonObject {
                put("pkg", ext.pkg)
                put("sources", buildJsonArray { ext.sources.forEach { add(sourceJson(it as CatalogueSource)) } })
            }
        }
        "sources" -> buildJsonArray { Loader.sources.values.forEach { add(sourceJson(it as CatalogueSource)) } }
        "popular" -> mangasPageJson(source().getPopularManga(page()))
        "latest" -> mangasPageJson(source().getLatestUpdates(page()))
        "search" -> {
            val src = source()
            mangasPageJson(src.getSearchManga(page(), str("query"), src.getFilterList()))
        }
        "manga" -> {
            val manga = mangaFrom(req["manga"]!!.jsonObject)
            val update = source().getMangaUpdate(manga, emptyList(), fetchDetails = true, fetchChapters = true)
            buildJsonObject {
                put("manga", mangaJson(update.manga))
                put("chapters", buildJsonArray { update.chapters.forEach { add(chapterJson(it)) } })
            }
        }
        "pages" -> {
            val pages = source().getPageList(chapterFrom(req["chapter"]!!.jsonObject))
            buildJsonArray { pages.forEach { add(pageJson(it)) } }
        }
        "image" -> {
            // Descarga con el cliente de la extensión (sus headers e interceptores, p. ej. para desordenar imágenes)
            val src = source() as? HttpSource ?: error("La fuente no es HttpSource")
            val page = pageFrom(req["page"]!!.jsonObject)
            if (page.imageUrl == null) page.imageUrl = src.getImageUrl(page)
            val out = File(str("out")).apply { parentFile?.mkdirs() }
            src.client.newCall(src.hostImageRequest(page)).awaitSuccess().use { resp ->
                resp.body.byteStream().use { input -> out.outputStream().use { input.copyTo(it) } }
                buildJsonObject {
                    put("path", out.absolutePath)
                    put("contentType", resp.header("Content-Type"))
                    put("bytes", out.length())
                }
            }
        }
        else -> error("op desconocida: $op")
    }
}

private fun sourceJson(s: CatalogueSource) = buildJsonObject {
    // id como string: es un Long de 64 bits y JSON/JS pierden precisión con números grandes
    put("id", s.id.toString())
    put("name", s.name)
    put("lang", s.lang)
    put("baseUrl", (s as? HttpSource)?.baseUrl)
    put("supportsLatest", s.supportsLatest)
}

private fun mangasPageJson(p: MangasPage) = buildJsonObject {
    put("hasNextPage", p.hasNextPage)
    put("mangas", buildJsonArray { p.mangas.forEach { add(mangaJson(it)) } })
}

private fun mangaJson(m: SManga) = buildJsonObject {
    put("url", m.url)
    put("title", m.title)
    put("thumbnail_url", m.thumbnail_url)
    put("author", m.author)
    put("artist", m.artist)
    put("status", m.status)
    put("description", m.description)
    put("genre", m.genre)
    put("initialized", m.initialized)
    put("memo", m.memo)
}

private fun chapterJson(c: SChapter) = buildJsonObject {
    put("url", c.url)
    put("name", c.name)
    put("chapter_number", c.chapter_number)
    put("scanlator", c.scanlator)
    put("date_upload", c.date_upload)
    put("memo", c.memo)
}

private fun pageJson(p: Page) = buildJsonObject {
    put("index", p.index)
    put("url", p.url)
    put("imageUrl", p.imageUrl)
}

private fun JsonObject.s(key: String) = this[key]?.jsonPrimitive?.contentOrNull

private fun mangaFrom(o: JsonObject) = SManga.create().apply {
    url = o.s("url") ?: error("manga sin url")
    title = o.s("title") ?: ""
    thumbnail_url = o.s("thumbnail_url")
    (o["memo"] as? JsonObject)?.let { memo = it }
}

private fun chapterFrom(o: JsonObject) = SChapter.create().apply {
    url = o.s("url") ?: error("capítulo sin url")
    name = o.s("name") ?: ""
    chapter_number = o["chapter_number"]?.jsonPrimitive?.floatOrNull ?: -1f
    (o["memo"] as? JsonObject)?.let { memo = it }
}

private fun pageFrom(o: JsonObject) = Page(
    index = o["index"]?.jsonPrimitive?.int ?: 0,
    url = o.s("url") ?: "",
    imageUrl = o.s("imageUrl"),
)
