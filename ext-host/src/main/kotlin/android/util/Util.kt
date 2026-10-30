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
