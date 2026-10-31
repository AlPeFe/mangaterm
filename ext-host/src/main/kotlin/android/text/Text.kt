package android.text

interface Editable : CharSequence

interface TextWatcher {
    fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int)
    fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int)
    fun afterTextChanged(s: Editable?)
}

interface Spanned : CharSequence

/** Texto sin estilos: basta para lo que hacen las extensiones (leer el texto y medirlo). */
class SpannedString(private val text: String) : Spanned, CharSequence by text {
    override fun toString() = text
}

object Html {
    const val FROM_HTML_MODE_LEGACY = 0
    const val FROM_HTML_MODE_COMPACT = 63

    @JvmStatic
    fun fromHtml(source: String?, flags: Int): Spanned {
        val doc = org.jsoup.Jsoup.parse((source ?: "").replace(Regex("(?i)<br\\s*/?>"), "\n").replace(Regex("(?i)</p>"), "\n\n"))
        return SpannedString(doc.wholeText().trim())
    }

    @JvmStatic
    fun fromHtml(source: String?): Spanned = fromHtml(source, FROM_HTML_MODE_LEGACY)
}

open class TextPaint() : android.graphics.Paint() {
    @JvmField var bgColor: Int = 0

    @JvmField var baselineShift: Int = 0

    @JvmField var linkColor: Int = 0

    @JvmField var density: Float = 1f

    constructor(flags: Int) : this() {
        setAntiAlias(flags and ANTI_ALIAS_FLAG != 0)
    }

    constructor(p: android.graphics.Paint) : this() {
        set(p)
    }
}

/** Texto en párrafos con saltos de línea por palabras, dibujado con java.awt. */
abstract class Layout protected constructor(
    private val text: CharSequence,
    private val paint: TextPaint,
    private val width: Int,
    private val align: Alignment,
    private val spacingMult: Float,
    private val spacingAdd: Float,
) {
    enum class Alignment { ALIGN_NORMAL, ALIGN_OPPOSITE, ALIGN_CENTER }

    private val lines: List<String> by lazy { wrap() }
    private val lineHeight: Float by lazy {
        val m = paint.getFontMetrics()
        (m.descent - m.ascent + m.leading) * spacingMult + spacingAdd
    }

    fun getText(): CharSequence = text

    fun getWidth(): Int = width

    fun getLineCount(): Int = lines.size

    fun getHeight(): Int = kotlin.math.ceil(lines.size * lineHeight).toInt()

    fun getLineWidth(line: Int): Float = paint.measureText(lines[line])

    fun draw(canvas: android.graphics.Canvas) {
        val ascent = paint.getFontMetrics().ascent
        lines.forEachIndexed { i, line ->
            val w = paint.measureText(line)
            val x = when (align) {
                Alignment.ALIGN_NORMAL -> 0f
                Alignment.ALIGN_CENTER -> (width - w) / 2
                Alignment.ALIGN_OPPOSITE -> width - w
            }
            if (line.isNotEmpty()) canvas.drawText(line, x, i * lineHeight - ascent, paint)
        }
    }

    private fun wrap(): List<String> {
        val out = mutableListOf<String>()
        for (paragraph in text.toString().split('\n')) {
            var current = ""
            for (word in paragraph.split(' ')) {
                val candidate = if (current.isEmpty()) word else "$current $word"
                if (current.isNotEmpty() && paint.measureText(candidate) > width) {
                    out += current
                    current = word
                } else {
                    current = candidate
                }
            }
            out += current
        }
        return out
    }
}

class StaticLayout(
    source: CharSequence,
    paint: TextPaint,
    width: Int,
    align: Alignment,
    spacingmult: Float,
    spacingadd: Float,
    includepad: Boolean,
) : Layout(source, paint, width, align, spacingmult, spacingadd) {
    class Builder private constructor(
        private val source: CharSequence,
        private val paint: TextPaint,
        private val width: Int,
    ) {
        private var align = Alignment.ALIGN_NORMAL
        private var mult = 1f
        private var add = 0f
        private var pad = true

        fun setAlignment(align: Alignment) = apply { this.align = align }

        fun setLineSpacing(add: Float, mult: Float) = apply {
            this.add = add
            this.mult = mult
        }

        fun setIncludePad(pad: Boolean) = apply { this.pad = pad }

        fun setBreakStrategy(strategy: Int) = this

        fun setHyphenationFrequency(frequency: Int) = this

        fun setMaxLines(lines: Int) = this

        fun setTextDirection(direction: Any?) = this

        fun build(): StaticLayout = StaticLayout(source, paint, width, align, mult, add, pad)

        companion object {
            @JvmStatic
            fun obtain(source: CharSequence, start: Int, end: Int, paint: TextPaint, width: Int): Builder =
                Builder(source.subSequence(start, end), paint, width)
        }
    }
}
