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
    companion object {
        @JvmField val MANUFACTURER: String = "Google"

        @JvmField val BRAND: String = "google"

        @JvmField val MODEL: String = "Pixel 8"

        @JvmField val DEVICE: String = "shiba"

        @JvmField val PRODUCT: String = "shiba"

        @JvmField val HARDWARE: String = "shiba"

        @JvmField val ID: String = "AP2A.240905.003"

        @JvmField val FINGERPRINT: String = "google/shiba/shiba:14/AP2A.240905.003/12231197:user/release-keys"
    }

    class VERSION {
        companion object {
            @JvmField val RELEASE: String = "14"

            @JvmField val SDK_INT: Int = 34
        }
    }
}
