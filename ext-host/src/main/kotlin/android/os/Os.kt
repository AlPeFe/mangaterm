@file:Suppress("unused")

package android.os

class Bundle

object SystemClock {
    private val start = System.nanoTime()

    @JvmStatic
    fun elapsedRealtime(): Long = (System.nanoTime() - start) / 1_000_000

    @JvmStatic
    fun uptimeMillis(): Long = elapsedRealtime()
}

class Build {
    class VERSION {
        companion object {
            @JvmField val RELEASE: String = "14"

            @JvmField val SDK_INT: Int = 34
        }
    }
}
