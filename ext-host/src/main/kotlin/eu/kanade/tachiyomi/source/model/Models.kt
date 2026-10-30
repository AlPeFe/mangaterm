package eu.kanade.tachiyomi.source.model

import android.net.Uri
import kotlinx.serialization.json.JsonObject

// Implementación real de los modelos de keiyoushi/extensions-lib 1.6.
// Los nombres y firmas tienen que coincidir byte a byte con la librería original.

class MangasPage(val mangas: List<SManga>, val hasNextPage: Boolean) {
    @Deprecated("MangasPage is now a regular class")
    operator fun component1(): List<SManga> = mangas

    @Deprecated("MangasPage is now a regular class")
    operator fun component2(): Boolean = hasNextPage

    @Deprecated("MangasPage is now a regular class")
    fun copy(mangas: List<SManga> = this.mangas, hasNextPage: Boolean = this.hasNextPage) = MangasPage(mangas, hasNextPage)
}

class Page(
    val index: Int,
    val url: String = "",
    var imageUrl: String? = null,
    var uri: Uri? = null,
)

class SMangaUpdate(val manga: SManga, val chapters: List<SChapter>)

enum class UpdateStrategy {
    ALWAYS_UPDATE,
    ONLY_FETCH_ONCE,
}

@Suppress("PropertyName")
interface SChapter {
    var url: String
    var name: String
    var chapter_number: Float
    var scanlator: String?
    var date_upload: Long
    var memo: JsonObject

    companion object {
        fun create(): SChapter = SChapterImpl()
    }
}

@Suppress("PropertyName")
interface SManga {
    var url: String
    var title: String
    var thumbnail_url: String?
    var artist: String?
    var author: String?
    var status: Int
    var description: String?
    var genre: String?
    var update_strategy: UpdateStrategy
    var memo: JsonObject
    var initialized: Boolean

    companion object {
        const val UNKNOWN = 0
        const val ONGOING = 1
        const val COMPLETED = 2
        const val LICENSED = 3
        const val PUBLISHING_FINISHED = 4
        const val CANCELLED = 5
        const val ON_HIATUS = 6

        fun create(): SManga = SMangaImpl()
    }
}

class SChapterImpl : SChapter {
    override var url = ""
    override var name = ""
    override var chapter_number = -1f
    override var scanlator: String? = null
    override var date_upload = 0L
    override var memo = JsonObject(emptyMap())
}

class SMangaImpl : SManga {
    override var url = ""
    override var title = ""
    override var thumbnail_url: String? = null
    override var artist: String? = null
    override var author: String? = null
    override var status = SManga.UNKNOWN
    override var description: String? = null
    override var genre: String? = null
    override var update_strategy = UpdateStrategy.ALWAYS_UPDATE
    override var memo = JsonObject(emptyMap())
    override var initialized = false
}
