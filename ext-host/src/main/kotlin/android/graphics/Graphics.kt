@file:Suppress("unused", "UNUSED_PARAMETER", "MemberVisibilityCanBePrivate")

package android.graphics

import java.awt.AlphaComposite
import java.awt.BasicStroke
import java.awt.Font
import java.awt.Graphics2D
import java.awt.RenderingHints
import java.awt.Shape
import java.awt.geom.AffineTransform
import java.awt.geom.RoundRectangle2D
import java.awt.image.BufferedImage
import java.io.ByteArrayInputStream
import java.io.File
import java.io.InputStream
import java.io.OutputStream
import javax.imageio.IIOImage
import javax.imageio.ImageIO
import javax.imageio.ImageWriteParam

// android.graphics sobre java.awt: lo que usan las extensiones para recomponer páginas que la web
// sirve troceadas o desordenadas (y para dibujar algún texto encima). Bitmap es un BufferedImage ARGB.

class Bitmap internal constructor(@JvmField internal val image: BufferedImage) {
    enum class Config { ALPHA_8, RGB_565, ARGB_4444, ARGB_8888, RGBA_F16, HARDWARE }

    enum class CompressFormat { JPEG, PNG, WEBP, WEBP_LOSSY, WEBP_LOSSLESS }

    val width: Int get() = image.width
    val height: Int get() = image.height
    val config: Config get() = Config.ARGB_8888
    val isMutable: Boolean get() = true
    var isRecycled: Boolean = false
        private set

    fun recycle() {
        isRecycled = true
    }

    fun getPixel(x: Int, y: Int): Int = image.getRGB(x, y)

    fun setPixel(x: Int, y: Int, color: Int) = image.setRGB(x, y, color)

    fun getPixels(pixels: IntArray, offset: Int, stride: Int, x: Int, y: Int, width: Int, height: Int) {
        image.getRGB(x, y, width, height, pixels, offset, stride)
    }

    fun setPixels(pixels: IntArray, offset: Int, stride: Int, x: Int, y: Int, width: Int, height: Int) {
        image.setRGB(x, y, width, height, pixels, offset, stride)
    }

    fun eraseColor(color: Int) {
        val g = image.createGraphics()
        g.composite = AlphaComposite.Src
        g.color = java.awt.Color(color, true)
        g.fillRect(0, 0, width, height)
        g.dispose()
    }

    fun copy(config: Config?, isMutable: Boolean): Bitmap = Bitmap(copyOf(image))

    /** Sin escritor de WebP en Java: WebP sale como PNG (el lector detecta el formato por el contenido). */
    fun compress(format: CompressFormat, quality: Int, stream: OutputStream): Boolean = try {
        if (format == CompressFormat.JPEG) {
            val rgb = BufferedImage(width, height, BufferedImage.TYPE_INT_RGB)
            rgb.createGraphics().apply {
                color = java.awt.Color.WHITE
                fillRect(0, 0, width, height)
                drawImage(image, 0, 0, null)
                dispose()
            }
            val writer = ImageIO.getImageWritersByFormatName("jpeg").next()
            val param = writer.defaultWriteParam.apply {
                compressionMode = ImageWriteParam.MODE_EXPLICIT
                compressionQuality = quality.coerceIn(1, 100) / 100f
            }
            ImageIO.createImageOutputStream(stream).use { out ->
                writer.output = out
                writer.write(null, IIOImage(rgb, null, null), param)
            }
            writer.dispose()
            true
        } else {
            ImageIO.write(image, "png", stream)
        }
    } catch (e: Exception) {
        false
    }

    companion object {
        @JvmStatic
        fun createBitmap(width: Int, height: Int, config: Config?): Bitmap =
            Bitmap(BufferedImage(width.coerceAtLeast(1), height.coerceAtLeast(1), BufferedImage.TYPE_INT_ARGB))

        @JvmStatic
        fun createBitmap(source: Bitmap): Bitmap = source.copy(null, true)

        @JvmStatic
        fun createBitmap(source: Bitmap, x: Int, y: Int, width: Int, height: Int): Bitmap =
            Bitmap(copyOf(source.image.getSubimage(x, y, width, height)))

        @JvmStatic
        fun createBitmap(source: Bitmap, x: Int, y: Int, width: Int, height: Int, m: Matrix?, filter: Boolean): Bitmap {
            val sub = createBitmap(source, x, y, width, height)
            if (m == null || m.transform.isIdentity) return sub
            val bounds = m.transform.createTransformedShape(java.awt.Rectangle(0, 0, width, height)).bounds
            val out = createBitmap(bounds.width, bounds.height, Config.ARGB_8888)
            val g = out.graphics()
            g.drawImage(sub.image, AffineTransform.getTranslateInstance(-bounds.x.toDouble(), -bounds.y.toDouble()).apply { concatenate(m.transform) }, null)
            g.dispose()
            return out
        }

        @JvmStatic
        fun createScaledBitmap(source: Bitmap, width: Int, height: Int, filter: Boolean): Bitmap {
            val out = createBitmap(width, height, Config.ARGB_8888)
            val g = out.graphics()
            g.drawImage(source.image, 0, 0, width, height, null)
            g.dispose()
            return out
        }

        internal fun of(image: BufferedImage): Bitmap =
            Bitmap(if (image.type == BufferedImage.TYPE_INT_ARGB) image else copyOf(image))

        private fun copyOf(src: BufferedImage): BufferedImage {
            val out = BufferedImage(src.width, src.height, BufferedImage.TYPE_INT_ARGB)
            out.createGraphics().apply {
                composite = AlphaComposite.Src
                drawImage(src, 0, 0, null)
                dispose()
            }
            return out
        }
    }

    internal fun graphics(): Graphics2D = image.createGraphics().apply {
        setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BILINEAR)
        setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
        setRenderingHint(RenderingHints.KEY_TEXT_ANTIALIASING, RenderingHints.VALUE_TEXT_ANTIALIAS_ON)
        setRenderingHint(RenderingHints.KEY_RENDERING, RenderingHints.VALUE_RENDER_QUALITY)
    }
}

object BitmapFactory {
    class Options {
        @JvmField var inMutable: Boolean = false

        @JvmField var inJustDecodeBounds: Boolean = false

        @JvmField var inSampleSize: Int = 1

        @JvmField var inPreferredConfig: Bitmap.Config? = Bitmap.Config.ARGB_8888

        @JvmField var outWidth: Int = -1

        @JvmField var outHeight: Int = -1

        @JvmField var outMimeType: String? = null
    }

    @JvmStatic
    fun decodeStream(stream: InputStream?): Bitmap? = decodeStream(stream, null, null)

    @JvmStatic
    fun decodeStream(stream: InputStream?, outPadding: Rect?, opts: Options?): Bitmap? {
        val image = try {
            stream?.let { ImageIO.read(it) }
        } catch (e: Exception) {
            null
        } ?: return null
        opts?.outWidth = image.width
        opts?.outHeight = image.height
        if (opts?.inJustDecodeBounds == true) return null
        return Bitmap.of(image)
    }

    @JvmStatic
    fun decodeByteArray(data: ByteArray, offset: Int, length: Int): Bitmap? = decodeByteArray(data, offset, length, null)

    @JvmStatic
    fun decodeByteArray(data: ByteArray, offset: Int, length: Int, opts: Options?): Bitmap? =
        decodeStream(ByteArrayInputStream(data, offset, length), null, opts)

    @JvmStatic
    fun decodeFile(path: String?): Bitmap? = path?.let { File(it).inputStream().use(::decodeStream) }
}

class Canvas(private val bitmap: Bitmap) {
    private val g = bitmap.graphics()
    private val saved = ArrayDeque<Pair<AffineTransform, Shape?>>()

    val width: Int get() = bitmap.width
    val height: Int get() = bitmap.height

    fun save(): Int {
        saved.addLast(g.transform to g.clip)
        return saved.size
    }

    fun restore() {
        val (transform, clip) = saved.removeLastOrNull() ?: return
        g.transform = transform
        g.clip = clip
    }

    fun restoreToCount(count: Int) {
        while (saved.size >= count && saved.isNotEmpty()) restore()
    }

    fun translate(dx: Float, dy: Float) = g.translate(dx.toDouble(), dy.toDouble())

    fun scale(sx: Float, sy: Float) = g.scale(sx.toDouble(), sy.toDouble())

    fun scale(sx: Float, sy: Float, px: Float, py: Float) {
        translate(px, py)
        scale(sx, sy)
        translate(-px, -py)
    }

    fun rotate(degrees: Float) = g.rotate(Math.toRadians(degrees.toDouble()))

    fun rotate(degrees: Float, px: Float, py: Float) = g.rotate(Math.toRadians(degrees.toDouble()), px.toDouble(), py.toDouble())

    fun concat(matrix: Matrix?) {
        matrix?.let { g.transform(it.transform) }
    }

    fun clipRect(left: Int, top: Int, right: Int, bottom: Int): Boolean {
        g.clipRect(left, top, right - left, bottom - top)
        return true
    }

    fun clipRect(rect: Rect): Boolean = clipRect(rect.left, rect.top, rect.right, rect.bottom)

    fun drawColor(color: Int) {
        val old = g.composite
        g.composite = AlphaComposite.Src
        g.color = java.awt.Color(color, true)
        val t = g.transform
        g.transform = AffineTransform()
        g.fillRect(0, 0, bitmap.width, bitmap.height)
        g.transform = t
        g.composite = old
    }

    fun drawRGB(r: Int, gr: Int, b: Int) = drawColor(Color.rgb(r, gr, b))

    fun drawBitmap(source: Bitmap, left: Float, top: Float, paint: Paint?) {
        g.drawImage(filtered(source, paint), AffineTransform.getTranslateInstance(left.toDouble(), top.toDouble()), null)
    }

    fun drawBitmap(source: Bitmap, src: Rect?, dst: Rect, paint: Paint?) =
        drawBitmap(source, src, dst.left.toFloat(), dst.top.toFloat(), dst.right.toFloat(), dst.bottom.toFloat(), paint)

    fun drawBitmap(source: Bitmap, src: Rect?, dst: RectF, paint: Paint?) =
        drawBitmap(source, src, dst.left, dst.top, dst.right, dst.bottom, paint)

    private fun drawBitmap(source: Bitmap, src: Rect?, l: Float, t: Float, r: Float, b: Float, paint: Paint?) {
        val s = src ?: Rect(0, 0, source.width, source.height)
        g.drawImage(
            filtered(source, paint),
            Math.round(l), Math.round(t), Math.round(r), Math.round(b),
            s.left, s.top, s.right, s.bottom,
            null,
        )
    }

    fun drawBitmap(source: Bitmap, matrix: Matrix, paint: Paint?) {
        g.drawImage(filtered(source, paint), matrix.transform, null)
    }

    fun drawRect(left: Float, top: Float, right: Float, bottom: Float, paint: Paint) =
        paint.draw(g, java.awt.geom.Rectangle2D.Float(left, top, right - left, bottom - top))

    fun drawRect(rect: RectF, paint: Paint) = drawRect(rect.left, rect.top, rect.right, rect.bottom, paint)

    fun drawRect(rect: Rect, paint: Paint) =
        drawRect(rect.left.toFloat(), rect.top.toFloat(), rect.right.toFloat(), rect.bottom.toFloat(), paint)

    fun drawRoundRect(rect: RectF, rx: Float, ry: Float, paint: Paint) =
        paint.draw(g, RoundRectangle2D.Float(rect.left, rect.top, rect.width(), rect.height(), rx * 2, ry * 2))

    fun drawText(text: String, x: Float, y: Float, paint: Paint) {
        val layout = java.awt.font.TextLayout(text, paint.font(), g.fontRenderContext)
        val dx = when (paint.textAlign) {
            Paint.Align.LEFT -> 0f
            Paint.Align.CENTER -> -layout.advance / 2
            Paint.Align.RIGHT -> -layout.advance
        }
        paint.draw(g, layout.getOutline(AffineTransform.getTranslateInstance((x + dx).toDouble(), y.toDouble())))
    }

    /** Aplica el ColorMatrixColorFilter del Paint, si lo hay. */
    private fun filtered(source: Bitmap, paint: Paint?): BufferedImage {
        val filter = paint?.colorFilter as? ColorMatrixColorFilter ?: return source.image
        val m = filter.matrix.array
        val (w, h) = source.width to source.height
        val px = source.image.getRGB(0, 0, w, h, null, 0, w)
        for (i in px.indices) {
            val c = px[i]
            val (a, r, gr, b) = listOf(c ushr 24 and 255, c shr 16 and 255, c shr 8 and 255, c and 255).map { it.toFloat() }
            fun ch(row: Int) = (m[row * 5] * r + m[row * 5 + 1] * gr + m[row * 5 + 2] * b + m[row * 5 + 3] * a + m[row * 5 + 4]).toInt().coerceIn(0, 255)
            px[i] = (ch(3) shl 24) or (ch(0) shl 16) or (ch(1) shl 8) or ch(2)
        }
        return BufferedImage(w, h, BufferedImage.TYPE_INT_ARGB).apply { setRGB(0, 0, w, h, px, 0, w) }
    }
}

open class Paint() {
    enum class Style { FILL, STROKE, FILL_AND_STROKE }

    enum class Join { MITER, ROUND, BEVEL }

    enum class Cap { BUTT, ROUND, SQUARE }

    enum class Align { LEFT, CENTER, RIGHT }

    class FontMetrics {
        @JvmField var top: Float = 0f

        @JvmField var ascent: Float = 0f

        @JvmField var descent: Float = 0f

        @JvmField var bottom: Float = 0f

        @JvmField var leading: Float = 0f
    }

    private var color = 0xFF000000.toInt()
    private var style = Style.FILL
    private var strokeWidth = 0f
    private var strokeJoin = Join.MITER
    private var textSize = 12f
    private var textScaleX = 1f
    private var typeface: Typeface? = null
    private var antiAlias = false
    private var filterBitmap = false
    internal var colorFilter: ColorFilter? = null
        private set
    internal var textAlign = Align.LEFT
        private set

    constructor(flags: Int) : this() {
        antiAlias = flags and ANTI_ALIAS_FLAG != 0
        filterBitmap = flags and FILTER_BITMAP_FLAG != 0
    }

    constructor(paint: Paint) : this() {
        set(paint)
    }

    fun set(src: Paint) {
        color = src.color
        style = src.style
        strokeWidth = src.strokeWidth
        strokeJoin = src.strokeJoin
        textSize = src.textSize
        textScaleX = src.textScaleX
        typeface = src.typeface
        antiAlias = src.antiAlias
        filterBitmap = src.filterBitmap
        colorFilter = src.colorFilter
        textAlign = src.textAlign
    }

    fun getColor(): Int = color

    fun setColor(color: Int) {
        this.color = color
    }

    fun setARGB(a: Int, r: Int, g: Int, b: Int) = setColor(Color.argb(a, r, g, b))

    fun getAlpha(): Int = color ushr 24

    fun setAlpha(a: Int) {
        color = (a.coerceIn(0, 255) shl 24) or (color and 0xFFFFFF)
    }

    fun getStyle(): Style = style

    fun setStyle(style: Style) {
        this.style = style
    }

    fun getStrokeWidth(): Float = strokeWidth

    fun setStrokeWidth(width: Float) {
        strokeWidth = width
    }

    fun setStrokeJoin(join: Join) {
        strokeJoin = join
    }

    fun setStrokeCap(cap: Cap) {}

    fun getTextSize(): Float = textSize

    fun setTextSize(size: Float) {
        textSize = size
    }

    fun getTextScaleX(): Float = textScaleX

    fun setTextScaleX(scale: Float) {
        textScaleX = scale
    }

    fun setTextAlign(align: Align) {
        textAlign = align
    }

    fun getTypeface(): Typeface? = typeface

    fun setTypeface(typeface: Typeface?): Typeface? {
        this.typeface = typeface
        return typeface
    }

    fun isAntiAlias(): Boolean = antiAlias

    fun setAntiAlias(aa: Boolean) {
        antiAlias = aa
    }

    fun setFilterBitmap(filter: Boolean) {
        filterBitmap = filter
    }

    fun setDither(dither: Boolean) {}

    fun setFakeBoldText(fake: Boolean) {}

    fun getColorFilter(): ColorFilter? = colorFilter

    fun setColorFilter(filter: ColorFilter?): ColorFilter? {
        colorFilter = filter
        return filter
    }

    fun measureText(text: String): Float =
        if (text.isEmpty()) 0f else java.awt.font.TextLayout(text, font(), FRC).advance

    fun measureText(text: CharSequence, start: Int, end: Int): Float = measureText(text.subSequence(start, end).toString())

    fun getFontMetrics(): FontMetrics = FontMetrics().also(::getFontMetrics)

    fun getFontMetrics(metrics: FontMetrics?): Float {
        val lm = font().getLineMetrics("Ag", FRC)
        metrics?.apply {
            ascent = -lm.ascent
            descent = lm.descent
            top = -lm.ascent * 1.15f
            bottom = lm.descent * 1.15f
            leading = lm.leading
        }
        return lm.leading + lm.ascent + lm.descent
    }

    fun getFontSpacing(): Float = getFontMetrics(null)

    internal fun font(): Font {
        val base = (typeface ?: Typeface.DEFAULT).font.deriveFont(textSize)
        return if (textScaleX == 1f) base else base.deriveFont(AffineTransform.getScaleInstance(textScaleX.toDouble(), 1.0))
    }

    /** Rellena y/o perfila `shape` según el estilo. */
    internal fun draw(g: Graphics2D, shape: Shape) {
        g.color = java.awt.Color(color, true)
        if (style != Style.STROKE) g.fill(shape)
        if (style != Style.FILL) {
            val join = when (strokeJoin) {
                Join.MITER -> BasicStroke.JOIN_MITER
                Join.ROUND -> BasicStroke.JOIN_ROUND
                Join.BEVEL -> BasicStroke.JOIN_BEVEL
            }
            g.stroke = BasicStroke(strokeWidth.coerceAtLeast(1f), BasicStroke.CAP_ROUND, join)
            g.draw(shape)
        }
    }

    companion object {
        const val ANTI_ALIAS_FLAG = 1
        const val FILTER_BITMAP_FLAG = 2
        const val DITHER_FLAG = 4

        internal val FRC = java.awt.font.FontRenderContext(null, true, true)
    }
}

class Typeface private constructor(internal val font: Font) {
    val isBold: Boolean get() = font.isBold

    companion object {
        const val NORMAL = 0
        const val BOLD = 1
        const val ITALIC = 2
        const val BOLD_ITALIC = 3

        @JvmField val DEFAULT = Typeface(Font(Font.SANS_SERIF, Font.PLAIN, 12))

        @JvmField val DEFAULT_BOLD = Typeface(Font(Font.SANS_SERIF, Font.BOLD, 12))

        @JvmField val SANS_SERIF = Typeface(Font(Font.SANS_SERIF, Font.PLAIN, 12))

        @JvmField val SERIF = Typeface(Font(Font.SERIF, Font.PLAIN, 12))

        @JvmField val MONOSPACE = Typeface(Font(Font.MONOSPACED, Font.PLAIN, 12))

        @JvmStatic
        fun create(family: Typeface?, style: Int): Typeface = Typeface((family ?: DEFAULT).font.deriveFont(awtStyle(style)))

        @JvmStatic
        fun create(family: String?, style: Int): Typeface = Typeface(Font(family ?: Font.SANS_SERIF, awtStyle(style), 12))

        @JvmStatic
        fun defaultFromStyle(style: Int): Typeface = create(DEFAULT, style)

        @JvmStatic
        fun createFromFile(file: File): Typeface = try {
            Typeface(Font.createFont(Font.TRUETYPE_FONT, file))
        } catch (e: Exception) {
            DEFAULT
        }

        @JvmStatic
        fun createFromFile(path: String): Typeface = createFromFile(File(path))

        private fun awtStyle(style: Int) = (if (style and BOLD != 0) Font.BOLD else 0) or (if (style and ITALIC != 0) Font.ITALIC else 0)
    }
}

object Color {
    const val BLACK = 0xFF000000.toInt()
    const val DKGRAY = 0xFF444444.toInt()
    const val GRAY = 0xFF888888.toInt()
    const val LTGRAY = 0xFFCCCCCC.toInt()
    const val WHITE = 0xFFFFFFFF.toInt()
    const val RED = 0xFFFF0000.toInt()
    const val GREEN = 0xFF00FF00.toInt()
    const val BLUE = 0xFF0000FF.toInt()
    const val YELLOW = 0xFFFFFF00.toInt()
    const val CYAN = 0xFF00FFFF.toInt()
    const val MAGENTA = 0xFFFF00FF.toInt()
    const val TRANSPARENT = 0

    private val names = mapOf(
        "black" to BLACK, "darkgray" to DKGRAY, "gray" to GRAY, "lightgray" to LTGRAY, "white" to WHITE,
        "red" to RED, "green" to GREEN, "blue" to BLUE, "yellow" to YELLOW, "cyan" to CYAN, "magenta" to MAGENTA,
        "transparent" to TRANSPARENT,
    )

    @JvmStatic
    fun parseColor(color: String): Int {
        if (color.startsWith("#")) {
            val hex = color.substring(1).toLong(16)
            return when (color.length) {
                7 -> (hex or 0xFF000000).toInt()
                9 -> hex.toInt()
                else -> throw IllegalArgumentException("Unknown color")
            }
        }
        return names[color.lowercase()] ?: throw IllegalArgumentException("Unknown color")
    }

    @JvmStatic
    fun rgb(red: Int, green: Int, blue: Int): Int = argb(255, red, green, blue)

    @JvmStatic
    fun argb(alpha: Int, red: Int, green: Int, blue: Int): Int = (alpha shl 24) or (red shl 16) or (green shl 8) or blue

    @JvmStatic
    fun alpha(color: Int): Int = color ushr 24

    @JvmStatic
    fun red(color: Int): Int = color shr 16 and 0xFF

    @JvmStatic
    fun green(color: Int): Int = color shr 8 and 0xFF

    @JvmStatic
    fun blue(color: Int): Int = color and 0xFF
}

class Rect() {
    @JvmField var left = 0

    @JvmField var top = 0

    @JvmField var right = 0

    @JvmField var bottom = 0

    constructor(left: Int, top: Int, right: Int, bottom: Int) : this() {
        set(left, top, right, bottom)
    }

    constructor(r: Rect) : this(r.left, r.top, r.right, r.bottom)

    fun set(left: Int, top: Int, right: Int, bottom: Int) {
        this.left = left
        this.top = top
        this.right = right
        this.bottom = bottom
    }

    fun set(src: Rect) = set(src.left, src.top, src.right, src.bottom)

    fun width(): Int = right - left

    fun height(): Int = bottom - top

    fun isEmpty(): Boolean = left >= right || top >= bottom

    fun offset(dx: Int, dy: Int) = set(left + dx, top + dy, right + dx, bottom + dy)

    override fun toString() = "Rect($left, $top - $right, $bottom)"
}

class RectF() {
    @JvmField var left = 0f

    @JvmField var top = 0f

    @JvmField var right = 0f

    @JvmField var bottom = 0f

    constructor(left: Float, top: Float, right: Float, bottom: Float) : this() {
        set(left, top, right, bottom)
    }

    constructor(r: Rect) : this(r.left.toFloat(), r.top.toFloat(), r.right.toFloat(), r.bottom.toFloat())

    fun set(left: Float, top: Float, right: Float, bottom: Float) {
        this.left = left
        this.top = top
        this.right = right
        this.bottom = bottom
    }

    fun width(): Float = right - left

    fun height(): Float = bottom - top
}

class Matrix() {
    internal var transform = AffineTransform()

    constructor(src: Matrix?) : this() {
        src?.let { transform = AffineTransform(it.transform) }
    }

    fun reset() = transform.setToIdentity()

    fun isIdentity(): Boolean = transform.isIdentity

    fun setScale(sx: Float, sy: Float) = transform.setToScale(sx.toDouble(), sy.toDouble())

    fun setTranslate(dx: Float, dy: Float) = transform.setToTranslation(dx.toDouble(), dy.toDouble())

    fun setRotate(degrees: Float) = transform.setToRotation(Math.toRadians(degrees.toDouble()))

    // post*: la nueva transformación se aplica después de la actual
    private fun post(t: AffineTransform): Boolean {
        transform.preConcatenate(t)
        return true
    }

    private fun pre(t: AffineTransform): Boolean {
        transform.concatenate(t)
        return true
    }

    fun postScale(sx: Float, sy: Float): Boolean = post(AffineTransform.getScaleInstance(sx.toDouble(), sy.toDouble()))

    fun postScale(sx: Float, sy: Float, px: Float, py: Float): Boolean = post(
        AffineTransform.getTranslateInstance(px.toDouble(), py.toDouble()).apply {
            scale(sx.toDouble(), sy.toDouble())
            translate(-px.toDouble(), -py.toDouble())
        },
    )

    fun postTranslate(dx: Float, dy: Float): Boolean = post(AffineTransform.getTranslateInstance(dx.toDouble(), dy.toDouble()))

    fun postRotate(degrees: Float): Boolean = post(AffineTransform.getRotateInstance(Math.toRadians(degrees.toDouble())))

    fun postRotate(degrees: Float, px: Float, py: Float): Boolean =
        post(AffineTransform.getRotateInstance(Math.toRadians(degrees.toDouble()), px.toDouble(), py.toDouble()))

    fun preScale(sx: Float, sy: Float): Boolean = pre(AffineTransform.getScaleInstance(sx.toDouble(), sy.toDouble()))

    fun preTranslate(dx: Float, dy: Float): Boolean = pre(AffineTransform.getTranslateInstance(dx.toDouble(), dy.toDouble()))

    fun preRotate(degrees: Float): Boolean = pre(AffineTransform.getRotateInstance(Math.toRadians(degrees.toDouble())))
}

open class ColorFilter

class ColorMatrix() {
    internal var array = floatArrayOf(1f, 0f, 0f, 0f, 0f, 0f, 1f, 0f, 0f, 0f, 0f, 0f, 1f, 0f, 0f, 0f, 0f, 0f, 1f, 0f)

    constructor(src: FloatArray) : this() {
        set(src)
    }

    fun set(src: FloatArray) {
        array = src.copyOf(20)
    }

    fun getArray(): FloatArray = array

    fun setSaturation(sat: Float) {
        val inv = 1 - sat
        val (r, g, b) = Triple(0.213f * inv, 0.715f * inv, 0.072f * inv)
        array = floatArrayOf(r + sat, g, b, 0f, 0f, r, g + sat, b, 0f, 0f, r, g, b + sat, 0f, 0f, 0f, 0f, 0f, 1f, 0f)
    }
}

class ColorMatrixColorFilter(internal val matrix: ColorMatrix) : ColorFilter() {
    constructor(array: FloatArray) : this(ColorMatrix(array))
}
